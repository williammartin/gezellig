pub struct RoomState {
    participants: Vec<String>,
}

impl RoomState {
    pub fn new() -> Self {
        Self {
            participants: Vec::new(),
        }
    }

    pub fn join(&mut self, name: String) {
        if !self.participants.contains(&name) {
            self.participants.push(name);
        }
    }

    pub fn leave(&mut self, name: &str) {
        self.participants.retain(|p| p != name);
    }

    pub fn participants(&self) -> &[String] {
        &self.participants
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
}
