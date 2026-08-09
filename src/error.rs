use pinocchio::error::ProgramError;

#[repr(u32)]
pub enum LabyrinthError {
    InvalidConfig = 0x4C_41_42_01,
    InvalidPhase,
    InvalidRoster,
    UnauthorizedSession,
    RequestAlreadyPending,
    RandomnessAlreadyResolved,
    InvalidVrfCallback,
    InvalidRoom,
    RoomAlreadyResolved,
    InvalidEvent,
    InvalidWinnerSet,
    FriendlyFireDisabled,
    MatchNotTerminal,
    EventLogFull,
    EventBatchFull,
    EventSequenceMismatch,
    PortalNotReady,
    AlreadyRequested,
    DuplicateEvent,
    AlreadyDelegated,
    DelegationIncomplete,
}

impl From<LabyrinthError> for ProgramError {
    fn from(value: LabyrinthError) -> Self {
        ProgramError::Custom(value as u32)
    }
}
