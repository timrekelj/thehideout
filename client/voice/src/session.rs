//! The connection state machine. It only decides which transitions are real and which
//! event each one produces, so a repeated or out-of-order signal from the SDK never reaches
//! the UI twice.

use crate::VoiceEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Idle,
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Debug)]
pub struct StateMachine {
    state: ConnState,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self {
            state: ConnState::Idle,
        }
    }
}

impl StateMachine {
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// Anything but `Idle`: there is a room (or an attempt at one) to tear down.
    pub fn is_active(&self) -> bool {
        self.state != ConnState::Idle
    }

    pub fn begin_connect(&mut self) -> Option<VoiceEvent> {
        if self.state != ConnState::Idle {
            return None;
        }
        self.state = ConnState::Connecting;
        Some(VoiceEvent::Connecting)
    }

    pub fn connected(&mut self) -> Option<VoiceEvent> {
        match self.state {
            ConnState::Connecting | ConnState::Reconnecting => {
                self.state = ConnState::Connected;
                Some(VoiceEvent::Connected)
            }
            _ => None,
        }
    }

    pub fn reconnecting(&mut self) -> Option<VoiceEvent> {
        if self.state != ConnState::Connected {
            return None;
        }
        self.state = ConnState::Reconnecting;
        Some(VoiceEvent::Reconnecting)
    }

    pub fn disconnected(&mut self, reason: impl Into<String>) -> Option<VoiceEvent> {
        if self.state == ConnState::Idle {
            return None;
        }
        self.state = ConnState::Idle;
        Some(VoiceEvent::Disconnected {
            reason: reason.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_emits_each_event_once() {
        let mut s = StateMachine::default();
        assert_eq!(s.begin_connect(), Some(VoiceEvent::Connecting));
        assert_eq!(s.begin_connect(), None, "already connecting");
        assert_eq!(s.connected(), Some(VoiceEvent::Connected));
        assert_eq!(s.connected(), None);
        assert_eq!(s.reconnecting(), Some(VoiceEvent::Reconnecting));
        assert_eq!(s.reconnecting(), None);
        assert_eq!(s.connected(), Some(VoiceEvent::Connected));
        assert_eq!(
            s.disconnected("client"),
            Some(VoiceEvent::Disconnected {
                reason: "client".into()
            })
        );
        assert_eq!(s.disconnected("again"), None);
        assert!(!s.is_active());
    }

    #[test]
    fn failed_attempt_goes_back_to_idle() {
        let mut s = StateMachine::default();
        s.begin_connect();
        assert!(s.is_active());
        assert!(s.disconnected("timeout").is_some());
        assert_eq!(s.state(), ConnState::Idle);
        assert_eq!(s.begin_connect(), Some(VoiceEvent::Connecting));
    }

    #[test]
    fn signals_out_of_order_are_ignored() {
        let mut s = StateMachine::default();
        assert_eq!(s.connected(), None);
        assert_eq!(s.reconnecting(), None);
        assert_eq!(s.disconnected("x"), None);
        s.begin_connect();
        assert_eq!(s.reconnecting(), None, "not connected yet");
    }
}
