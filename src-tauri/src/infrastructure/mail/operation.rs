//! 单次邮件任务的截止时间与取消；阻塞协议调用必须等待线程真正退出。

use crate::infrastructure::error::InfraError;
use std::future::Future;
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct OperationState {
    socket: Option<TcpStream>,
    cancelled: bool,
}

struct OperationInner {
    deadline: Instant,
    state: Mutex<OperationState>,
    cancelled: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone)]
pub struct MailOperation(Arc<OperationInner>);

/// 调用方 Future 被丢弃时发出取消；后台任务负责继续等待退出。
pub struct CancelOnDrop(Option<MailOperation>);

impl CancelOnDrop {
    pub fn disarm(mut self) {
        self.0.take();
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(operation) = &self.0 {
            operation.cancel();
        }
    }
}

fn interrupted() -> InfraError {
    InfraError::Network {
        message: "邮件任务已取消或超时".into(),
    }
}

impl MailOperation {
    pub fn new(budget: Duration) -> Self {
        let (cancelled, _) = tokio::sync::watch::channel(false);
        Self(Arc::new(OperationInner {
            deadline: Instant::now() + budget,
            state: Mutex::new(OperationState {
                socket: None,
                cancelled: false,
            }),
            cancelled,
        }))
    }

    pub fn cancel_on_drop(&self) -> CancelOnDrop {
        CancelOnDrop(Some(self.clone()))
    }

    pub fn phase_deadline(&self, budget: Duration) -> Instant {
        self.0.deadline.min(Instant::now() + budget)
    }

    pub fn deadline(&self) -> Instant {
        self.0.deadline
    }

    pub fn cancel(&self) {
        let socket = {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            state.cancelled = true;
            state.socket.take()
        };
        self.0.cancelled.send_replace(true);
        if let Some(socket) = socket {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub fn checkpoint(&self) -> Result<(), InfraError> {
        if Instant::now() >= self.0.deadline {
            self.cancel();
        }
        if self
            .0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancelled
        {
            return Err(interrupted());
        }
        Ok(())
    }

    pub fn monitor_socket(&self, socket: &TcpStream) -> Result<(), InfraError> {
        if let Err(error) = self.checkpoint() {
            let _ = socket.shutdown(Shutdown::Both);
            return Err(error);
        }
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.cancelled {
            let _ = socket.shutdown(Shutdown::Both);
            return Err(interrupted());
        }
        // 单次任务只使用一个协议连接，防止错误覆盖仍需取消的旧连接。
        if state.socket.is_some() {
            return Err(InfraError::Conflict {
                message: "邮件任务不能重叠使用多个连接".into(),
            });
        }
        state.socket = Some(socket.try_clone().map_err(|_| InfraError::Network {
            message: "无法监控邮件连接".into(),
        })?);
        Ok(())
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.0.cancelled.subscribe();
        if !*receiver.borrow_and_update() {
            let _ = receiver.changed().await;
        }
    }

    pub async fn wait<T>(
        &self,
        future: impl Future<Output = Result<T, InfraError>>,
    ) -> Result<T, InfraError> {
        self.wait_until(self.0.deadline, future).await
    }

    pub async fn wait_until<T>(
        &self,
        deadline: Instant,
        future: impl Future<Output = Result<T, InfraError>>,
    ) -> Result<T, InfraError> {
        if Instant::now() >= deadline {
            self.cancel();
        }
        self.checkpoint()?;
        let result = tokio::select! {
            result = future => result,
            _ = self.cancelled() => Err(interrupted()),
            _ = tokio::time::sleep_until(deadline.min(self.0.deadline).into()) => {
                self.cancel();
                Err(interrupted())
            }
        };
        if Instant::now() >= deadline {
            self.cancel();
        }
        self.checkpoint()?;
        result
    }

    pub async fn run_blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, InfraError> + Send + 'static,
    ) -> Result<T, InfraError> {
        self.run_blocking_until(self.0.deadline, work).await
    }

    pub async fn run_blocking_until<T: Send + 'static>(
        &self,
        deadline: Instant,
        work: impl FnOnce() -> Result<T, InfraError> + Send + 'static,
    ) -> Result<T, InfraError> {
        if Instant::now() >= deadline {
            self.cancel();
        }
        self.checkpoint()?;
        let guard = self.cancel_on_drop();
        let operation = self.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            operation.checkpoint()?;
            work()
        });
        let result = tokio::select! {
            result = &mut worker => result,
            _ = self.cancelled() => {
                self.cancel();
                let _ = worker.await;
                return Err(interrupted());
            }
            _ = tokio::time::sleep_until(deadline.min(self.0.deadline).into()) => {
                self.cancel();
                let _ = worker.await;
                return Err(interrupted());
            }
        };
        if Instant::now() >= deadline {
            self.cancel();
        }
        self.checkpoint()?;
        guard.disarm();
        result.map_err(|_| InfraError::Network {
            message: "邮件阻塞任务异常退出".into(),
        })?
    }

    /// 超时/取消时仍等待阻塞线程退出，并保留线程自身已经取得的结果。
    ///
    /// 仅用于有副作用的远端写入（如 APPEND）：调用方必须自行区分「线程未开始」
    /// 与「开始后结果未知」。
    pub async fn run_blocking_preserving_result<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, InfraError> + Send + 'static,
    ) -> Result<T, InfraError> {
        let deadline = self.0.deadline;
        if Instant::now() >= deadline {
            self.cancel();
        }
        self.checkpoint()?;
        let guard = self.cancel_on_drop();
        let operation = self.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            operation.checkpoint()?;
            work()
        });
        let interrupted = tokio::select! {
            result = &mut worker => {
                if Instant::now() >= deadline {
                    self.cancel();
                }
                guard.disarm();
                return result.map_err(|_| InfraError::Network {
                    message: "邮件阻塞任务异常退出".into(),
                })?;
            }
            _ = self.cancelled() => {
                self.cancel();
                true
            }
            _ = tokio::time::sleep_until(deadline.min(self.0.deadline).into()) => {
                self.cancel();
                true
            }
        };
        debug_assert!(interrupted);
        // 必须先等待线程真正退出，再返回它保留的结果。
        let result = worker.await.map_err(|_| InfraError::Network {
            message: "邮件阻塞任务异常退出".into(),
        })?;
        guard.disarm();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[tokio::test]
    async fn preserving_result_waits_for_worker_and_keeps_success_after_deadline() {
        let operation = MailOperation::new(Duration::from_millis(50));
        let control = operation.clone();
        let start = Instant::now();
        let result: Result<Option<bool>, InfraError> = operation
            .run_blocking_preserving_result(move || {
                // 模拟 APPEND 已成功、后续清理被取消。
                control.cancel();
                Ok::<_, InfraError>(Some(true))
            })
            .await;
        assert_eq!(result.unwrap(), Some(true));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(operation.checkpoint().is_err());
    }

    #[tokio::test]
    async fn preserving_result_keeps_unknown_after_cancel() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let control = operation.clone();
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            control.cancel();
        });
        let result: Result<Option<bool>, InfraError> = operation
            .run_blocking_preserving_result(|| {
                std::thread::sleep(Duration::from_millis(300));
                Ok::<_, InfraError>(None)
            })
            .await;
        canceller.await.unwrap();
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn deadline_interrupts_established_socket_and_drains_worker() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server_socket, _) = listener.accept().unwrap();
        // 与真实协议路径一样，底层 IO 必须有限等待；Windows shutdown 本身也可能等待读取。
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let operation = MailOperation::new(Duration::from_millis(50));
        operation.monitor_socket(&socket).unwrap();
        let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel();
        let start = Instant::now();
        let result = operation
            .run_blocking(move || {
                let _ = socket.read(&mut [0; 1]);
                exited_tx.send(()).unwrap();
                Ok(())
            })
            .await;
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(exited_rx.try_recv().is_ok(), "超时返回前线程必须已退出");
        assert!(operation.checkpoint().is_err());
    }

    #[tokio::test]
    async fn cancellation_is_sticky_and_prevents_a_late_connection_or_async_result() {
        let operation = MailOperation::new(Duration::from_secs(2));
        operation.cancel();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        assert!(operation.monitor_socket(&socket).is_err());
        assert!(operation.wait(async { Ok(42) }).await.is_err());
        assert!(operation.run_blocking(|| Ok(42)).await.is_err());
        tokio::time::timeout(Duration::from_millis(100), operation.cancelled())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn established_imap_command_cannot_keep_reading_after_cancel() {
        use std::io::{BufRead, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_socket, _) = listener.accept().unwrap();
        let (selected_tx, selected_rx) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(server_socket);
            let mut command = String::new();
            reader.read_line(&mut command).unwrap();
            assert!(command.starts_with("a1 LOGIN "));
            reader.get_mut().write_all(b"a1 OK logged in\r\n").unwrap();
            command.clear();
            reader.read_line(&mut command).unwrap();
            assert_eq!(command, "a2 SELECT \"INBOX\"\r\n");
            selected_tx.send(()).unwrap();
            reader
                .get_mut()
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let result = reader.read(&mut [0; 1]);
            assert!(
                matches!(result, Ok(0))
                    || result.is_err_and(|error| error.kind() != std::io::ErrorKind::TimedOut)
            );
        });
        let operation = MailOperation::new(Duration::from_secs(5));
        operation.monitor_socket(&socket).unwrap();
        let control = operation.clone();
        let (exited_tx, mut exited_rx) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            control
                .run_blocking(move || {
                    let mut session = imap::Client::new(socket)
                        .login("user", "secret")
                        .map_err(|_| interrupted())?;
                    let result = session.select("INBOX");
                    exited_tx.send(()).unwrap();
                    result.map(|_| ()).map_err(|_| interrupted())
                })
                .await
        });
        selected_rx.await.unwrap();
        operation.cancel();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), caller)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(exited_rx.try_recv().is_ok());
        server.join().unwrap();
    }
}
