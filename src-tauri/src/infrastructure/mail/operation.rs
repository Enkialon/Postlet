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

/// 一次异步远端写入在取消/截止下的结果边界。
///
/// 远端写入必须区分「调用没开始」「调用自身已返回」「调用被打断」：
/// 只有前两者能得出可复用的结论，被打断的写入结果未知。
pub enum CallOutcome<T> {
    /// 调用自身返回了协议结果（即使同一时刻已发生取消/截止）
    Returned(T),
    /// 取消或截止发生在调用开始前：可证明未提交
    NotStarted,
    /// 调用已开始后被打断：结果未知
    Aborted,
}

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

    /// 异步远端写入的结果边界：协议自身结果优先，取消/截止只阻止尚未开始的调用。
    ///
    /// 与 `run_blocking_preserving_result` 对称：已经取得的接受回执不会被随后到达的
    /// 取消或截止时间覆盖；已开始后被取消则丢弃本次调用并报告结果未知，禁止自动重发。
    pub async fn wait_preserving_result<T>(
        &self,
        future: impl Future<Output = Result<T, InfraError>>,
    ) -> CallOutcome<Result<T, InfraError>> {
        if Instant::now() >= self.0.deadline {
            self.cancel();
        }
        if self.checkpoint().is_err() {
            return CallOutcome::NotStarted;
        }
        let mut future = std::pin::pin!(future);
        tokio::select! {
            // 先轮询调用再观察取消：已返回的协议结果不能被同时到达的取消覆盖。
            biased;
            result = &mut future => CallOutcome::Returned(result),
            _ = self.cancelled() => {
                self.cancel();
                CallOutcome::Aborted
            }
            _ = tokio::time::sleep_until(self.0.deadline.into()) => {
                self.cancel();
                CallOutcome::Aborted
            }
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mail::{SendOutcome, SendResult, SendStage};
    use std::io::Read;
    use std::net::TcpListener;

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

    #[tokio::test]
    async fn preserving_async_result_keeps_receipt_returned_while_cancelled() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let control = operation.clone();
        // 取消与回执同时到达：已取得的协议结果不得被取消覆盖。
        let outcome = operation
            .wait_preserving_result(async move {
                control.cancel();
                Ok::<_, InfraError>(SendResult::new(SendOutcome::Accepted, SendStage::Data))
            })
            .await;
        match outcome {
            CallOutcome::Returned(Ok(result)) => assert_eq!(result.outcome, SendOutcome::Accepted),
            _ => panic!("已取得的接受回执必须保留"),
        }
        assert!(operation.checkpoint().is_err(), "取消事实仍必须生效");
    }

    #[tokio::test]
    async fn preserving_async_result_does_not_start_a_cancelled_call() {
        let operation = MailOperation::new(Duration::from_secs(5));
        operation.cancel();
        let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = started.clone();
        let outcome = operation
            .wait_preserving_result(async move {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok::<_, InfraError>(42)
            })
            .await;
        assert!(matches!(outcome, CallOutcome::NotStarted));
        assert!(!started.load(std::sync::atomic::Ordering::SeqCst), "未开始的调用不得执行");
    }

    #[tokio::test]
    async fn preserving_async_result_reports_expired_budget_as_not_started() {
        let operation = MailOperation::new(Duration::ZERO);
        let outcome = operation
            .wait_preserving_result(async { Ok::<_, InfraError>(42) })
            .await;
        assert!(matches!(outcome, CallOutcome::NotStarted));
    }

    #[tokio::test]
    async fn preserving_async_result_reports_started_call_as_unknown() {
        let operation = MailOperation::new(Duration::from_secs(5));
        let control = operation.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let canceller = tokio::spawn(async move {
            started_rx.await.unwrap();
            control.cancel();
        });
        let outcome = operation
            .wait_preserving_result(async move {
                started_tx.send(()).unwrap();
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok::<_, InfraError>(42)
            })
            .await;
        canceller.await.unwrap();
        assert!(matches!(outcome, CallOutcome::Aborted));
    }

    #[tokio::test]
    async fn preserving_async_result_reports_call_own_error_after_deadline() {
        let operation = MailOperation::new(Duration::from_millis(50));
        let control = operation.clone();
        let outcome = operation
            .wait_preserving_result(async move {
                control.cancel();
                Err::<i32, _>(InfraError::Auth {
                    message: "SMTP 认证被拒绝".into(),
                })
            })
            .await;
        match outcome {
            CallOutcome::Returned(Err(error)) => {
                assert!(matches!(error, InfraError::Auth { .. }))
            }
            _ => panic!("协议自身的确定失败不得退化为未知"),
        }
    }
}
