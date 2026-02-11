pub struct RoomState {
    participants: Vec<String>,
    dj: Option<String>,
}

impl RoomState {
    pub fn new() -> Self {
        Self {
            participants: Vec::new(),
            dj: None,
        }
    }

    pub fn join(&mut self, name: String) {
        if !self.participants.contains(&name) {
            self.participants.push(name);
        }
    }

    pub fn leave(&mut self, name: &str) {
        self.participants.retain(|p| p != name);
        if self.dj.as_deref() == Some(name) {
            self.dj = None;
        }
    }

    pub fn participants(&self) -> &[String] {
        &self.participants
    }

    pub fn become_dj(&mut self, name: String) -> Result<(), String> {
        if !self.participants.contains(&name) {
            return Err("Must be in the room to become DJ".to_string());
        }
        self.dj = Some(name);
        Ok(())
    }

    pub fn stop_dj(&mut self, name: &str) {
        if self.dj.as_deref() == Some(name) {
            self.dj = None;
        }
    }

    pub fn current_dj(&self) -> Option<&str> {
        self.dj.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_room_has_no_participants() {
        let room = RoomState::new();
        assert!(room.participants().is_empty());
    }

    #[test]
    fn join_adds_participant() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        assert_eq!(room.participants(), &["Alice"]);
    }

    #[test]
    fn join_is_idempotent() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        room.join("Alice".to_string());
        assert_eq!(room.participants(), &["Alice"]);
    }

    #[test]
    fn leave_removes_participant() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        room.leave("Alice");
        assert!(room.participants().is_empty());
    }

    #[test]
    fn leave_nonexistent_is_noop() {
        let mut room = RoomState::new();
        room.leave("Nobody");
        assert!(room.participants().is_empty());
    }

    #[test]
    fn multiple_participants() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        room.join("Bob".to_string());
        assert_eq!(room.participants(), &["Alice", "Bob"]);
        room.leave("Alice");
        assert_eq!(room.participants(), &["Bob"]);
    }

    #[test]
    fn no_dj_initially() {
        let room = RoomState::new();
        assert!(room.current_dj().is_none());
    }

    #[test]
    fn become_dj() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        assert!(room.become_dj("Alice".to_string()).is_ok());
        assert_eq!(room.current_dj(), Some("Alice"));
    }

    #[test]
    fn cannot_become_dj_without_joining() {
        let mut room = RoomState::new();
        assert!(room.become_dj("Alice".to_string()).is_err());
    }

    #[test]
    fn stop_dj() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        assert!(room.become_dj("Alice".to_string()).is_ok());
        room.stop_dj("Alice");
        assert!(room.current_dj().is_none());
    }

    #[test]
    fn leaving_clears_dj() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        assert!(room.become_dj("Alice".to_string()).is_ok());
        room.leave("Alice");
        assert!(room.current_dj().is_none());
    }

    #[test]
    fn new_dj_replaces_old() {
        let mut room = RoomState::new();
        room.join("Alice".to_string());
        room.join("Bob".to_string());
        assert!(room.become_dj("Alice".to_string()).is_ok());
        assert!(room.become_dj("Bob".to_string()).is_ok());
        assert_eq!(room.current_dj(), Some("Bob"));
    }
}
