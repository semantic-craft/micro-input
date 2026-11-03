//! Offline verification harness, compiled only with synthetic-backend. No FFI/native writer.
use super::*;
use mi_model::apply::PreparedCandidate;
use std::collections::BTreeSet;
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    NotSent,
    DeviceStateUnknown,
    Verified,
}
/// Owns consumed plan IDs for one synthetic session. A failure is never retried automatically.
pub struct SyntheticWriter {
    session: Arc<DeviceSession>,
    consumed: AsyncMutex<BTreeSet<String>>,
}
// If the caller drops an in-flight verification future, do not leave a reusable
// connection with a possibly changed device and an unobserved response.
struct MutationGuard<'a> {
    session: &'a DeviceSession,
    sent: bool,
    verified: bool,
}
impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        if self.sent && !self.verified {
            self.session.disconnect();
        }
    }
}
impl SyntheticWriter {
    pub fn new(session: Arc<DeviceSession>) -> Result<Self, SessionError> {
        if session.link.backend() != mi_transport::LinkBackend::SyntheticNumbered {
            return Err(
                ProtoError::Unverified("native configuration writing remains disabled").into(),
            );
        }
        Ok(Self {
            session,
            consumed: AsyncMutex::new(BTreeSet::new()),
        })
    }
    pub async fn apply(&self, plan: &PreparedCandidate) -> Outcome {
        if plan.session_id() != self.session.id {
            return Outcome::NotSent;
        }
        let mut consumed = self.consumed.lock().await;
        if !consumed.insert(plan.operation_id().into()) {
            return Outcome::NotSent;
        }
        let mut guard = MutationGuard {
            session: &self.session,
            sent: false,
            verified: false,
        };
        let result = self
            .session
            .guarded(async {
                let mut q = self.session.queue.lock().await;
                let current = self.session.read_files_locked(&mut q).await?;
                if current.keymap.bytes.as_ref() != plan.original()
                    || current.smart_actions.as_ref().map(|f| f.bytes.as_ref())
                        != plan.smart_actions()
                {
                    return Err(SessionError::Config("synthetic baseline changed".into()));
                }
                let chunks = plan.candidate().len().div_ceil(3072);
                let reads = plan.candidate().len().div_ceil(3072)
                    + plan.smart_actions().map_or(0, |b| b.len().div_ceil(3072))
                    + 2;
                if chunks == 0 || usize::from(q.next_id) + chunks + reads > 999 {
                    return Err(SessionError::Config("request budget insufficient".into()));
                }
                if let Some(snapshot) = self.session.snapshot.lock().unwrap().as_mut() {
                    snapshot.stale = true;
                }
                for offset in (0..plan.candidate().len()).step_by(3072) {
                    let encoded =
                        mi_proto::mutation::keymap_chunk(plan.candidate(), offset, q.next_id)?;
                    guard.sent = true;
                    let reply = self
                        .session
                        .exchange_encoded(&mut q, "fs.writebin", encoded)
                        .await?;
                    mi_proto::mutation::synthetic_ack(&reply)?;
                }
                let actual = self.session.read_files_locked(&mut q).await?;
                if actual.keymap.bytes.as_ref() != plan.candidate()
                    || actual.smart_actions.as_ref().map(|f| f.bytes.as_ref())
                        != plan.smart_actions()
                {
                    return Err(SessionError::Config("synthetic readback differs".into()));
                }
                Ok(())
            })
            .await;
        match result {
            Ok(()) => {
                guard.verified = true;
                Outcome::Verified
            }
            Err(_) if guard.sent => {
                self.session.disconnect();
                Outcome::DeviceStateUnknown
            }
            Err(_) => Outcome::NotSent,
        }
    }
}
