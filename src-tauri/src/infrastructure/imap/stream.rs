//! 对协议库呈现阻塞 IO，但每次底层读写有限等待并检查取消/绝对截止时间。

use crate::infrastructure::error::InfraError;
use crate::infrastructure::mail::operation::MailOperation;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IO_CHECK_INTERVAL: Duration = Duration::from_millis(100);

pub struct DeadlineTcp {
    socket: TcpStream,
    operation: MailOperation,
    phase_deadline: Arc<Mutex<Instant>>,
    idle_read_deadline: Option<Instant>,
}

impl DeadlineTcp {
    pub fn new(
        socket: TcpStream,
        operation: &MailOperation,
        phase_deadline: Arc<Mutex<Instant>>,
    ) -> Result<Self, InfraError> {
        socket
            .set_read_timeout(Some(IO_CHECK_INTERVAL))
            .map_err(|_| InfraError::Network {
                message: "无法设置 IMAP 读取超时".into(),
            })?;
        socket
            .set_write_timeout(Some(IO_CHECK_INTERVAL))
            .map_err(|_| InfraError::Network {
                message: "无法设置 IMAP 写入超时".into(),
            })?;
        operation.monitor_socket(&socket)?;
        Ok(Self {
            socket,
            operation: operation.clone(),
            phase_deadline,
            idle_read_deadline: None,
        })
    }

    fn check(&self, reading: bool) -> io::Result<()> {
        self.operation
            .checkpoint()
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "IMAP operation cancelled"))?;
        if Instant::now()
            >= *self
                .phase_deadline
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        {
            self.operation.cancel();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IMAP phase timeout",
            ));
        }
        if reading
            && self
                .idle_read_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "IMAP IDLE timeout"));
        }
        Ok(())
    }
}

impl Read for DeadlineTcp {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.check(true)?;
            match self.socket.read(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                result => return result,
            }
        }
    }
}

impl Write for DeadlineTcp {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.check(false)?;
            match self.socket.write(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.check(false)?;
        self.socket.flush()
    }
}

pub struct ImapStream(pub native_tls::TlsStream<DeadlineTcp>);

impl std::fmt::Debug for ImapStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImapStream")
    }
}

impl Read for ImapStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.get_ref().check(true)?;
        self.0.read(bytes)
    }
}

impl Write for ImapStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.get_ref().check(false)?;
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.get_ref().check(false)?;
        self.0.flush()
    }
}

impl imap::extensions::idle::SetReadTimeout for ImapStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::error::Result<()> {
        // None 只移除 IDLE 等待期限；不能清除任务的绝对截止时间与底层有限等待。
        self.0.get_mut().idle_read_deadline = timeout.map(|duration| Instant::now() + duration);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn temporary_io_timeout_is_retried_but_phase_deadline_is_absolute() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server_socket, _) = listener.accept().unwrap();
        let operation = MailOperation::new(Duration::from_secs(5));
        let phase = Arc::new(Mutex::new(Instant::now() + Duration::from_millis(350)));
        let mut stream = DeadlineTcp::new(socket, &operation, phase).unwrap();
        assert_eq!(
            stream.socket.read_timeout().unwrap(),
            Some(IO_CHECK_INTERVAL)
        );
        assert_eq!(
            stream.socket.write_timeout().unwrap(),
            Some(IO_CHECK_INTERVAL)
        );
        let start = Instant::now();
        assert_eq!(
            stream.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() >= Duration::from_millis(350));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(operation.checkpoint().is_err());
    }

    #[test]
    fn idle_timeout_reset_keeps_operation_cancellation_and_short_io_waits() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server_socket, _) = listener.accept().unwrap();
        let operation = MailOperation::new(Duration::from_secs(5));
        let phase = Arc::new(Mutex::new(operation.phase_deadline(Duration::from_secs(5))));
        let mut stream = DeadlineTcp::new(socket, &operation, phase).unwrap();
        stream.idle_read_deadline = Some(Instant::now());
        assert!(stream.read(&mut [0; 1]).is_err());
        assert!(operation.checkpoint().is_ok(), "普通 IDLE 超时不取消任务");
        stream.idle_read_deadline = None;
        let control = operation.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            control.cancel();
        });
        let start = Instant::now();
        assert!(stream.read(&mut [0; 1]).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        canceller.join().unwrap();
    }
}
