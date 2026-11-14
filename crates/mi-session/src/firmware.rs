use super::*;

impl DeviceSession {
    fn require_firmware_bootloader_device(&self) -> Result<(), SessionError> {
        if self.is_disconnected() {
            return Err(self.disconnection_error());
        }
        if self.link.backend() != mi_transport::LinkBackend::MacHidapi267Numbered
            || !self.has_exclusive_vendor_link()
            || !self
                .firmware
                .lock()
                .unwrap()
                .as_deref()
                .is_some_and(mi_model::is_supported_bootloader_firmware)
        {
            return Err(ProtoError::Unverified(
                "firmware bootloader requires verified Creator Micro 2 USB exclusive session",
            )
            .into());
        }
        Ok(())
    }

    fn firmware_bootloader_request(id: u16) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": id, "method": "sys.bootloader", "params": null
        }))
        .expect("fixed bootloader JSON is serializable")
    }

    pub(super) fn validate_firmware_bootloader_request(
        &self,
        id: u16,
        encoded: &[u8],
    ) -> Result<(), SessionError> {
        self.require_firmware_bootloader_device()?;
        if !self.firmware_bootloader_requested.load(Ordering::SeqCst)
            || encoded != Self::firmware_bootloader_request(id)
        {
            return Err(
                ProtoError::Invalid("bootloader request is not the typed command".into()).into(),
            );
        }
        Ok(())
    }

    /// Input 1.1.0 device-kit K:4330–4338, 4406–4412, 4628–4637.
    /// A successful reply acknowledges the request only. The caller must observe
    /// the bootloader separately; timeout/disconnect must never trigger replay.
    pub async fn enter_firmware_bootloader(&self) -> Result<(), SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            self.require_firmware_bootloader_device()?;
            if self
                .firmware_bootloader_requested
                .swap(true, Ordering::SeqCst)
            {
                return Err(SessionError::Config(
                    "本次连接已经请求进入固件更新模式；不会重发".into(),
                ));
            }
            let id = queue.next_id;
            let reply = self
                .exchange_authorized(
                    &mut queue,
                    "sys.bootloader",
                    Self::firmware_bootloader_request(id),
                    Some("sys.bootloader"),
                )
                .await?;
            mi_proto::mutation::validate_mutation_response_candidate(&reply, id)?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_proto::{Line, Report};
    use mi_transport::{LinkBackend, MockLink};
    use serde_json::json;

    struct NativeTestLink {
        inner: Arc<MockLink>,
        exclusive: bool,
    }
    #[async_trait::async_trait]
    impl HidLink for NativeTestLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }
        fn is_exclusive_vendor(&self) -> bool {
            self.exclusive
        }
        async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
            self.inner.write(report).await
        }
        async fn read(&self) -> Result<Vec<u8>, TransportError> {
            self.inner.read().await
        }
        fn close(&self) {
            self.inner.close();
        }
    }
    struct TestFramer;
    impl Framer for TestFramer {
        fn encode(&self, bytes: &[u8]) -> Result<Vec<Report>, ProtoError> {
            Ok(vec![bytes.to_vec()])
        }
        fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Line>, ProtoError> {
            Ok(vec![bytes.to_vec()])
        }
        fn clear(&mut self) {}
    }
    fn setup(
        exclusive: bool,
    ) -> (
        Arc<DeviceSession>,
        Arc<MockLink>,
        tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ) {
        let (link, rx) = MockLink::new();
        let session = DeviceSession::new(
            Arc::new(NativeTestLink {
                inner: link.clone(),
                exclusive,
            }),
            Box::new(TestFramer),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(100),
                interval: Duration::ZERO,
            },
        );
        (session, link, rx)
    }

    #[tokio::test]
    async fn observed_firmware_uses_identity_response_and_clears_on_disconnect() {
        let (session, link, mut rx) = setup(true);
        assert_eq!(session.observed_firmware_version(), None);
        let reading = session.clone();
        let task = tokio::spawn(async move { reading.read_identity_info().await });
        let request: Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
        link.inject
            .send(
                serde_json::to_vec(&json!({"id":request["id"],"result":{"version":"0.7.0-rc.16"}}))
                    .unwrap(),
            )
            .unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(
            session.observed_firmware_version().as_deref(),
            Some("0.7.0-rc.16")
        );
        assert_eq!(link.writes.lock().unwrap().len(), 1);
        session.shutdown().await;
        assert_eq!(session.observed_firmware_version(), None);
    }

    #[tokio::test]
    async fn bootloader_typed_command_is_exact_once_and_general_exit_stays_closed() {
        let (session, link, mut rx) = setup(true);
        *session.firmware.lock().unwrap() = Some("0.6.3".into());
        {
            let mut queue = session.queue.lock().await;
            let bytes = DeviceSession::firmware_bootloader_request(queue.next_id);
            assert!(session
                .exchange_encoded(&mut queue, "sys.bootloader", bytes)
                .await
                .is_err());
            assert!(link.writes.lock().unwrap().is_empty());
        }
        let task_session = session.clone();
        let task = tokio::spawn(async move { task_session.enter_firmware_bootloader().await });
        let bytes = rx.recv().await.unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            request,
            json!({"id":request["id"], "method":"sys.bootloader", "params":null})
        );
        link.inject
            .send(serde_json::to_vec(&json!({"id":request["id"],"result":null})).unwrap())
            .unwrap();
        task.await.unwrap().unwrap();
        assert!(session
            .validate_firmware_bootloader_request(
                1,
                br#"{"id":1,"method":"sys.bootloader","params":{"erase":true}}"#
            )
            .is_err());
        assert!(session
            .validate_firmware_bootloader_request(
                1,
                br#"{"id":1,"method":"sys.bootloader","method":"fs.remove","params":null}"#
            )
            .is_err());
        assert!(session.enter_firmware_bootloader().await.is_err());
        assert_eq!(link.writes.lock().unwrap().len(), 1);
        session.shutdown().await;
    }

    #[tokio::test]
    async fn bootloader_unknown_identity_or_shared_link_never_sends() {
        for (exclusive, version) in [
            (true, Some("0.7.0-rc.16")),
            (true, Some("0.7.0")),
            (true, Some("0.8.0")),
        ] {
            let (session, _link, _) = setup(exclusive);
            *session.firmware.lock().unwrap() = version.map(str::to_owned);
            assert!(
                session.require_firmware_bootloader_device().is_ok(),
                "bootloader should accept {version:?}"
            );
            session.shutdown().await;
        }
        for (exclusive, version) in [
            (true, None),
            (true, Some("0.6.3-rc.10")),
            (true, Some("0.7")),
            (true, Some("synthetic-0.6.3")),
            (false, Some("0.6.3")),
        ] {
            let (session, link, _) = setup(exclusive);
            *session.firmware.lock().unwrap() = version.map(str::to_owned);
            assert!(
                session.enter_firmware_bootloader().await.is_err(),
                "bootloader should reject exclusive={exclusive}, version={version:?}"
            );
            assert!(link.writes.lock().unwrap().is_empty());
            session.shutdown().await;
        }
    }

    #[tokio::test]
    async fn bootloader_timeout_does_not_allow_retry() {
        let (session, link, _rx) = setup(true);
        *session.firmware.lock().unwrap() = Some("0.6.3".into());
        assert!(matches!(
            session.enter_firmware_bootloader().await,
            Err(SessionError::Timeout)
        ));
        assert!(session.enter_firmware_bootloader().await.is_err());
        assert_eq!(link.writes.lock().unwrap().len(), 1);
        session.shutdown().await;
    }
}
