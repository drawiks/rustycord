use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU16,
};

use davey::{DaveSession, MediaType, ProposalsOperationType};
use tracing::warn;

#[derive(Debug, Clone)]
pub enum DaveAction {
    KeyPackage(Vec<u8>),
    CommitWelcome {
        commit: Vec<u8>,
        welcome: Option<Vec<u8>>,
    },
    Ready(u16),
    Invalid(u16),
}

pub struct Dave {
    session: Option<DaveSession>,
    version: u16,
    active_version: u16,
    pending: HashMap<u16, u16>,
    recognized: HashSet<u64>,
    external_sender: Option<Vec<u8>>,
    user_id: u64,
    channel_id: u64,
}

impl Dave {
    pub fn new(user_id: u64, channel_id: u64) -> Self {
        let mut recognized = HashSet::new();
        recognized.insert(user_id);
        Self {
            session: None,
            version: 0,
            active_version: 0,
            pending: HashMap::new(),
            recognized,
            external_sender: None,
            user_id,
            channel_id,
        }
    }

    pub fn set_external_sender(&mut self, data: &[u8]) {
        self.external_sender = Some(data.to_vec());
        if let Some(session) = &mut self.session
            && let Err(error) = session.set_external_sender(data)
        {
            warn!("dave external sender failed: {error:?}");
        }
    }

    pub fn client_connect(&mut self, user_id: u64) {
        self.recognized.insert(user_id);
    }

    pub fn client_disconnect(&mut self, user_id: u64) {
        self.recognized.remove(&user_id);
    }

    pub fn prepare_transition(&mut self, transition_id: u16, version: u16) -> Vec<DaveAction> {
        self.pending.insert(transition_id, version);
        if transition_id == 0 {
            self.execute_transition(0);
            return Vec::new();
        }
        if version == 0 {
            if let Some(session) = &mut self.session {
                session.set_passthrough_mode(true, Some(120));
            }
            return vec![DaveAction::Ready(transition_id)];
        }
        Vec::new()
    }

    pub fn execute_transition(&mut self, transition_id: u16) {
        let Some(new_version) = self.pending.remove(&transition_id) else {
            warn!("dave execute for unknown transition {transition_id}");
            return;
        };
        let old_version = self.active_version;
        self.version = new_version;
        self.active_version = new_version;
        if transition_id > 0
            && old_version == 0
            && new_version != 0
            && let Some(session) = &mut self.session
        {
            session.set_passthrough_mode(true, Some(10));
        }
    }

    pub fn prepare_epoch(&mut self, epoch: u32, version: u16) -> Vec<DaveAction> {
        self.version = version;
        if epoch != 1 {
            return Vec::new();
        }
        let Some(version) = NonZeroU16::new(version) else {
            if let Some(session) = &mut self.session {
                let _ = session.reset();
                session.set_passthrough_mode(true, Some(10));
            }
            return Vec::new();
        };

        if let Some(session) = &mut self.session {
            if let Err(error) = session.reinit(version, self.user_id, self.channel_id, None) {
                warn!("dave reinit failed: {error:?}");
                return Vec::new();
            }
        } else {
            match DaveSession::new(version, self.user_id, self.channel_id, None) {
                Ok(mut session) => {
                    if let Some(external) = &self.external_sender
                        && let Err(error) = session.set_external_sender(external)
                    {
                        warn!("dave external sender failed: {error:?}");
                    }
                    self.session = Some(session);
                }
                Err(error) => {
                    warn!("dave session init failed: {error:?}");
                    return Vec::new();
                }
            }
        }

        self.create_key_package()
    }

    fn create_key_package(&mut self) -> Vec<DaveAction> {
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        match session.create_key_package() {
            Ok(package) => vec![DaveAction::KeyPackage(package)],
            Err(error) => {
                warn!("dave key package failed: {error:?}");
                Vec::new()
            }
        }
    }

    fn reinit(&mut self) -> Vec<DaveAction> {
        let Some(version) = NonZeroU16::new(self.version) else {
            return Vec::new();
        };
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        if let Err(error) = session.reinit(version, self.user_id, self.channel_id, None) {
            warn!("dave reinit failed: {error:?}");
            return Vec::new();
        }
        self.create_key_package()
    }

    pub fn proposals(&mut self, operation: u8, data: &[u8]) -> Vec<DaveAction> {
        let operation = match operation {
            0 => ProposalsOperationType::APPEND,
            1 => ProposalsOperationType::REVOKE,
            other => {
                warn!("dave unknown proposals operation {other}");
                return Vec::new();
            }
        };
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        let users: Vec<u64> = self.recognized.iter().copied().collect();
        match session.process_proposals(operation, data, Some(&users)) {
            Ok(Some(result)) => vec![DaveAction::CommitWelcome {
                commit: result.commit,
                welcome: result.welcome,
            }],
            Ok(None) => Vec::new(),
            Err(error) => {
                warn!("dave proposals failed: {error:?}");
                Vec::new()
            }
        }
    }

    pub fn commit(&mut self, transition_id: u16, data: &[u8]) -> Vec<DaveAction> {
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        match session.process_commit(data) {
            Ok(true) if transition_id != 0 => {
                self.pending.insert(transition_id, self.version);
                vec![DaveAction::Ready(transition_id)]
            }
            Ok(_) => Vec::new(),
            Err(error) => {
                warn!("dave commit failed: {error:?}");
                let mut actions = vec![DaveAction::Invalid(transition_id)];
                actions.extend(self.reinit());
                actions
            }
        }
    }

    pub fn welcome(&mut self, transition_id: u16, data: &[u8]) -> Vec<DaveAction> {
        let Some(session) = &mut self.session else {
            return Vec::new();
        };
        match session.process_welcome(data, None) {
            Ok(()) if transition_id != 0 => {
                self.pending.insert(transition_id, self.version);
                vec![DaveAction::Ready(transition_id)]
            }
            Ok(()) => Vec::new(),
            Err(error) => {
                warn!("dave welcome failed: {error:?}");
                let mut actions = vec![DaveAction::Invalid(transition_id)];
                actions.extend(self.reinit());
                actions
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.is_ready())
    }

    #[allow(dead_code)] // used by the audio engine
    pub fn encrypt_opus(&mut self, packet: &[u8]) -> Option<Vec<u8>> {
        if self.active_version == 0 {
            return None;
        }
        let session = self.session.as_mut()?;
        if !session.can_encrypt() {
            return None;
        }
        match session.encrypt_opus(packet) {
            Ok(encrypted) => Some(encrypted.into_owned()),
            Err(error) => {
                warn!("dave encrypt failed: {error:?}");
                None
            }
        }
    }

    pub fn decrypt_opus(&mut self, user_id: u64, packet: &[u8]) -> Option<Vec<u8>> {
        let session = self.session.as_mut()?;
        if session.can_passthrough(user_id) {
            return None;
        }
        match session.decrypt(user_id, MediaType::AUDIO, packet) {
            Ok(plain) => Some(plain),
            Err(error) => {
                tracing::debug!("dave decrypt failed: {error:?}");
                None
            }
        }
    }
}
