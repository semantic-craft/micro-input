//! Verified `0.7.0-rc.16+` hardware capability RPC helpers gated behind exclusive vendor ownership.

use mi_proto::v070_capabilities::{
    decode_device_name_response, decode_joystick_deadzone_response, decode_mouse_config_response,
    decode_pm_config_response, decode_pm_state_response, decode_set_profile_layer_response,
    decode_set_sa_rgb_response, encode_read_device_name, encode_read_joystick_deadzone,
    encode_read_mouse_config, encode_read_pm_config, encode_read_pm_state, encode_set_device_name,
    encode_set_profile_layer, encode_set_sa_rgb, encode_write_joystick_deadzone,
    encode_write_mouse_config, encode_write_pm_config, DeviceNameInfo, JoystickDeadzone,
    MouseConfig, PmConfig, PmState, ProfileLayerTarget, SaRgbUpdate, DEVICE_NAME_METHOD,
    JOYSTICK_DEADZONE_METHOD, MAX_DEVICE_NAME_LEN, MOUSE_CONFIG_METHOD, PM_CONFIG_METHOD,
    PM_STATE_METHOD, PROFILE_LAYER_SET_METHOD, SA_RGB_METHOD,
};
use mi_proto::ProtoError;
use serde_json::Value;

use crate::{DeviceSession, SessionError};

impl DeviceSession {
    async fn exchange_v070<E>(
        &self,
        method: &'static str,
        encode: E,
    ) -> Result<(u16, Value), SessionError>
    where
        E: FnOnce(u16) -> Result<Vec<u8>, ProtoError>,
    {
        self.guarded(async {
            let mut q = self.queue.lock().await;
            let id = q.next_id;
            let encoded = if id >= 999 { Vec::new() } else { encode(id)? };
            let reply = self
                .exchange_authorized(&mut q, method, encoded, Some(method))
                .await?;
            Ok((id, reply))
        })
        .await
    }

    pub fn observed_battery_status(&self) -> (Option<u8>, Option<bool>) {
        *self.battery_status.lock().unwrap()
    }

    pub fn observed_profile_layer(&self) -> Option<(u8, u8)> {
        *self.profile_layer.lock().unwrap()
    }

    pub async fn read_mouse_config(&self) -> Result<MouseConfig, SessionError> {
        let (id, reply) = self
            .exchange_v070(MOUSE_CONFIG_METHOD, encode_read_mouse_config)
            .await?;
        Ok(decode_mouse_config_response(&reply, id)?)
    }

    pub async fn write_mouse_config(&self, cfg: &MouseConfig) -> Result<MouseConfig, SessionError> {
        cfg.validate()?;
        let (id, reply) = self
            .exchange_v070(MOUSE_CONFIG_METHOD, |id| encode_write_mouse_config(cfg, id))
            .await?;
        Ok(decode_mouse_config_response(&reply, id)?)
    }

    pub async fn read_joystick_deadzone(&self) -> Result<f64, SessionError> {
        let (id, reply) = self
            .exchange_v070(JOYSTICK_DEADZONE_METHOD, encode_read_joystick_deadzone)
            .await?;
        Ok(decode_joystick_deadzone_response(&reply, id)?.v)
    }

    pub async fn write_joystick_deadzone(&self, v: f64) -> Result<f64, SessionError> {
        let dz = JoystickDeadzone::new(v)?;
        let (id, reply) = self
            .exchange_v070(JOYSTICK_DEADZONE_METHOD, |id| {
                encode_write_joystick_deadzone(dz, id)
            })
            .await?;
        Ok(decode_joystick_deadzone_response(&reply, id)?.v)
    }

    pub async fn set_profile_layer_flashless(
        &self,
        p: Option<u8>,
        l: Option<u8>,
    ) -> Result<(), SessionError> {
        let target = ProfileLayerTarget { p, l };
        target.validate()?;
        let (id, reply) = self
            .exchange_v070(PROFILE_LAYER_SET_METHOD, |id| {
                encode_set_profile_layer(target, id)
            })
            .await?;
        decode_set_profile_layer_response(&reply, id)?;
        let mut guard = self.profile_layer.lock().unwrap();
        let current = *guard;
        let next_p = p.or_else(|| current.map(|(cp, _)| cp));
        let next_l = l.or_else(|| current.map(|(_, cl)| cl));
        if let Some((np, nl)) = next_p.zip(next_l) {
            *guard = Some((np, nl));
        }
        Ok(())
    }

    pub async fn read_pm_config(&self) -> Result<PmConfig, SessionError> {
        let (id, reply) = self
            .exchange_v070(PM_CONFIG_METHOD, encode_read_pm_config)
            .await?;
        Ok(decode_pm_config_response(&reply, id)?)
    }

    pub async fn write_pm_config(&self, cfg: &PmConfig) -> Result<PmConfig, SessionError> {
        cfg.validate()?;
        let (id, reply) = self
            .exchange_v070(PM_CONFIG_METHOD, |id| encode_write_pm_config(cfg, id))
            .await?;
        Ok(decode_pm_config_response(&reply, id)?)
    }

    pub async fn read_pm_state(&self) -> Result<PmState, SessionError> {
        let (id, reply) = self
            .exchange_v070(PM_STATE_METHOD, encode_read_pm_state)
            .await?;
        Ok(decode_pm_state_response(&reply, id)?)
    }

    pub async fn read_device_name(&self) -> Result<DeviceNameInfo, SessionError> {
        let (id, reply) = self
            .exchange_v070(DEVICE_NAME_METHOD, encode_read_device_name)
            .await?;
        let info = decode_device_name_response(&reply, id)?;
        *self.device_name_max_length.lock().unwrap() = Some(info.max_length);
        Ok(info)
    }

    pub async fn write_device_name(&self, name: &str) -> Result<DeviceNameInfo, SessionError> {
        let max_length = self
            .device_name_max_length
            .lock()
            .unwrap()
            .unwrap_or(MAX_DEVICE_NAME_LEN);
        let (id, reply) = self
            .exchange_v070(DEVICE_NAME_METHOD, |id| {
                encode_set_device_name(name, max_length, id)
            })
            .await?;
        let info = decode_device_name_response(&reply, id)?;
        *self.device_name_max_length.lock().unwrap() = Some(info.max_length);
        Ok(info)
    }

    pub async fn set_sa_rgb(&self, update: &SaRgbUpdate) -> Result<(), SessionError> {
        update.validate()?;
        let (id, reply) = self
            .exchange_v070(SA_RGB_METHOD, |id| encode_set_sa_rgb(update, id))
            .await?;
        Ok(decode_set_sa_rgb_response(&reply, id)?)
    }

    pub async fn reset_sa_rgb(&self, sa: Vec<u32>) -> Result<(), SessionError> {
        let reset = SaRgbUpdate::reset(sa);
        self.set_sa_rgb(&reset).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mi_proto::v070_capabilities::{MouseConfig, PmConfig, SaRgbEffect, SaRgbUpdate};
    use mi_proto::{Framer, NativeFramer, ProtoError};
    use mi_transport::{HidLink, LinkBackend, TransportError};
    use serde_json::{json, Value};

    use crate::{DeviceSession, ReadOnlySchema, SessionConfig, SessionError};

    struct TestFramer;
    impl Framer for TestFramer {
        fn encode(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![payload.to_vec()])
        }
        fn feed(&mut self, report: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![report.to_vec()])
        }
        fn clear(&mut self) {}
    }

    struct NativeVendorTestLink {
        tx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
        rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        closed: std::sync::atomic::AtomicBool,
        exclusive: bool,
        backend: LinkBackend,
    }

    impl NativeVendorTestLink {
        fn new(
            exclusive: bool,
            backend: LinkBackend,
        ) -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) {
            let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
            let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    tx: std::sync::Mutex::new(Some(in_tx)),
                    rx: tokio::sync::Mutex::new(in_rx),
                    out_tx,
                    closed: std::sync::atomic::AtomicBool::new(false),
                    exclusive,
                    backend,
                }),
                out_rx,
            )
        }

        fn push_reply(&self, value: Value) {
            if let Some(tx) = self.tx.lock().unwrap().as_ref() {
                let _ = tx.send(serde_json::to_vec(&value).unwrap());
            }
        }
    }

    #[async_trait::async_trait]
    impl HidLink for NativeVendorTestLink {
        fn is_exclusive_vendor(&self) -> bool {
            self.exclusive
        }
        fn backend(&self) -> LinkBackend {
            self.backend
        }
        async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
            if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(TransportError::Disconnected);
            }
            let _ = self.out_tx.send(report.to_vec());
            Ok(())
        }
        async fn read(&self) -> Result<Vec<u8>, TransportError> {
            let mut rx = self.rx.lock().await;
            rx.recv().await.ok_or(TransportError::Disconnected)
        }
        fn close(&self) {
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            self.tx.lock().unwrap().take();
        }
    }

    fn setup_session(
        exclusive: bool,
        backend: LinkBackend,
    ) -> (
        Arc<DeviceSession>,
        Arc<NativeVendorTestLink>,
        tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ) {
        let (link, out_rx) = NativeVendorTestLink::new(exclusive, backend);
        let session = DeviceSession::new(
            link.clone(),
            Box::new(TestFramer),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(200),
                interval: Duration::ZERO,
            },
        );
        (session, link, out_rx)
    }

    async fn read_identity(
        session: &DeviceSession,
        link: &NativeVendorTestLink,
        out_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        version: &str,
    ) {
        let fut = session.read_identity_info();
        tokio::pin!(fut);
        let req_bytes = tokio::select! {
            _ = &mut fut => panic!("read_identity_info returned before receiving reply"),
            b = out_rx.recv() => b.unwrap(),
        };
        let req: Value = serde_json::from_slice(&req_bytes).unwrap();
        link.push_reply(json!({
            "id": req["id"],
            "result": {
                "version": version,
                "profile_index": 3,
                "layer_index": 1,
                "battery": 84,
                "is_charging": true
            }
        }));
        let info = fut.await.unwrap();
        assert_eq!(info.version.as_deref(), Some(version));
        assert_eq!(info.battery, Some(84));
        assert_eq!(info.is_charging, Some(true));
        assert_eq!(session.observed_battery_status(), (Some(84), Some(true)));
        assert_eq!(session.observed_profile_layer(), Some((3, 1)));
    }

    #[tokio::test]
    async fn all_v070_capability_methods_succeed_on_exclusive_v070_link() {
        let (session, link, mut out_rx) = setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session, &link, &mut out_rx, "0.7.0-rc.16").await;

        let link_responder = link.clone();
        let responder = tokio::spawn(async move {
            while let Some(raw) = out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap();
                let result = match method {
                    "mouse.config" if req["params"].is_null() => {
                        serde_json::to_value(MouseConfig::default()).unwrap()
                    }
                    "mouse.config" => req["params"].clone(),
                    "joystick.deadzone" if req["params"].is_null() => json!({"v": 0.15}),
                    "joystick.deadzone" => req["params"].clone(),
                    "kb.pl.set" => json!({}),
                    "pm.config" if req["params"].is_null() => {
                        serde_json::to_value(PmConfig::default()).unwrap()
                    }
                    "pm.config" => req["params"].clone(),
                    "pm.state" => json!({
                        "state": "active",
                        "floor": "usb",
                        "base_floor": "host_sleep",
                        "line_powered": true,
                        "auto_sleep": true,
                        "sleep_after_s": 900,
                        "idle_s": 3,
                        "standby_s": 3600,
                        "credit_s": 0,
                        "holds": [{"owner": "usb.host", "floor": "usb", "age_ms": 1200}]
                    }),
                    "device.name" if req["params"].is_null() => json!({
                        "name": "Creator Micro 2",
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "device.name" => json!({
                        "name": req["params"]["name"],
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "kb.sa.rgb" => json!({}),
                    other => panic!("unexpected method {other}"),
                };
                link_responder.push_reply(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        });

        let mc = session.read_mouse_config().await.unwrap();
        assert_eq!(mc, MouseConfig::default());
        let mut custom_mc = mc;
        custom_mc.move_delta = 12;
        assert_eq!(
            session.write_mouse_config(&custom_mc).await.unwrap(),
            custom_mc
        );

        assert_eq!(session.read_joystick_deadzone().await.unwrap(), 0.15);
        assert_eq!(session.write_joystick_deadzone(0.22).await.unwrap(), 0.22);

        session
            .set_profile_layer_flashless(Some(3), Some(2))
            .await
            .unwrap();
        assert_eq!(session.observed_profile_layer(), Some((3, 2)));

        let pm = session.read_pm_config().await.unwrap();
        assert_eq!(pm, PmConfig::default());
        let updated_pm = PmConfig {
            auto_sleep: false,
            host_sleep: true,
            sleep_after_min: 30,
        };
        assert_eq!(
            session.write_pm_config(&updated_pm).await.unwrap(),
            updated_pm
        );

        let pm_state = session.read_pm_state().await.unwrap();
        assert_eq!(pm_state.usb_host_hold().unwrap().floor, "usb");

        let name_info = session.read_device_name().await.unwrap();
        assert_eq!(name_info.max_length, 15);
        let renamed = session.write_device_name("Micro-Studio").await.unwrap();
        assert_eq!(renamed.name, "Micro-Studio");

        let rgb = SaRgbUpdate {
            sa: vec![7057],
            c: Some(0x33AAFF),
            b: Some(0.8),
            s: Some(0.4),
            e: Some(SaRgbEffect::Breath.as_u8()),
        };
        session.set_sa_rgb(&rgb).await.unwrap();
        session.reset_sa_rgb(vec![7057]).await.unwrap();

        session.shutdown().await;
        responder.abort();
    }

    #[tokio::test]
    async fn v070_methods_fail_closed_on_v063_missing_version_or_non_exclusive_links_with_zero_frames(
    ) {
        // 1) Firmware 0.6.3 on exclusive MacHidapi267Numbered: must fail closed before sending.
        let (session_063, link_063, mut out_063) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session_063, &link_063, &mut out_063, "0.6.3").await;
        assert!(matches!(
            session_063.read_mouse_config().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_063
                .set_profile_layer_flashless(Some(3), Some(1))
                .await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_063.read_pm_state().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(out_063.try_recv().is_err(), "no frame may be sent on 0.6.3");
        session_063.shutdown().await;

        // 2) Unknown/missing firmware version before read_identity_info: must fail closed.
        let (session_missing, _link_missing, mut out_missing) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        assert!(matches!(
            session_missing.read_device_name().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(
            out_missing.try_recv().is_err(),
            "no frame may be sent without version"
        );
        session_missing.shutdown().await;

        // 3) Non-exclusive link even on 0.7.0-rc.16: must fail closed.
        let (session_shared, link_shared, mut out_shared) =
            setup_session(false, LinkBackend::MacHidapi267Numbered);
        read_identity(
            &session_shared,
            &link_shared,
            &mut out_shared,
            "0.7.0-rc.16",
        )
        .await;
        assert!(matches!(
            session_shared.read_pm_config().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_shared.write_joystick_deadzone(0.2).await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(
            out_shared.try_recv().is_err(),
            "no frame may be sent without exclusive vendor link"
        );
        session_shared.shutdown().await;

        // 4) Non-267 backend even on exclusive 0.7.0-rc.16: must fail closed.
        #[cfg(feature = "synthetic-backend")]
        {
            let (session_syn, link_syn, mut out_syn) =
                setup_session(true, LinkBackend::SyntheticNumbered);
            read_identity(&session_syn, &link_syn, &mut out_syn, "0.7.0-rc.16").await;
            assert!(matches!(
                session_syn.read_mouse_config().await,
                Err(SessionError::Protocol(ProtoError::Unverified(_)))
            ));
            assert!(
                out_syn.try_recv().is_err(),
                "no frame may be sent on non-267 backend"
            );
            session_syn.shutdown().await;
        }
        // Keep NativeFramer referenced cleanly so compiler doesn't complain if unused.
        let _ = NativeFramer::default();
    }

    #[tokio::test]
    async fn kb_pl_notification_updates_observed_profile_layer() {
        let (session, link, _out_rx) = setup_session(true, LinkBackend::MacHidapi267Numbered);
        let mut rx = session.notifications();
        assert_eq!(session.observed_profile_layer(), None);

        link.push_reply(json!({"m": "kb.pl", "p": {"p": 3, "l": 4}}));
        let received = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            received.notification,
            mi_proto::HostNotification::ProfileLayer(_)
        ));
        assert_eq!(session.observed_profile_layer(), Some((3, 4)));
        session.shutdown().await;
    }
}
