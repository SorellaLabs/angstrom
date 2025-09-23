//! Copied from Sigp Lighthouse

use std::{
    ops::Add,
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use crate::primitive::CHAIN_ID;

pub const INTERVALS_PER_SLOT: u64 = 3;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(u64);

/// A clock that reports the current slot.
///
/// The clock is not required to be monotonically increasing and may go
/// backwards.
pub trait SlotClock: Send + Sync + Sized + Clone {
    /// Creates a new system slot clock using the parameters for the staticly
    /// set `CHAIN_ID`
    fn new_default() -> Option<Self> {
        Self::new_with_chain_id(*CHAIN_ID.get().expect("CHAIN_ID has not been set yet"))
    }

    /// Creates a new system slot clock using the parameters for a specific
    /// chain id. Some chain ids will not have one
    fn new_with_chain_id(chain_id: u64) -> Option<Self>;

    /// Returns the slot at this present time.
    fn now(&self) -> Option<Slot>;

    /// Returns the slot at this present time if genesis has happened.
    /// Otherwise, returns the genesis slot. Returns `None` if there is an
    /// error reading the clock.
    fn now_or_genesis(&self) -> Option<Slot> {
        if self.is_prior_to_genesis()? { Some(self.genesis_slot()) } else { self.now() }
    }

    /// Indicates if the current time is prior to genesis time.
    ///
    /// Returns `None` if the system clock cannot be read.
    fn is_prior_to_genesis(&self) -> Option<bool>;

    /// Returns the present time as a duration since the UNIX epoch.
    ///
    /// Returns `None` if the present time is before the UNIX epoch (unlikely).
    fn now_duration(&self) -> Option<Duration>;

    /// Returns the slot of the given duration since the UNIX epoch.
    fn slot_of(&self, now: Duration) -> Option<Slot>;

    /// Returns the duration between slots
    fn slot_duration(&self) -> Duration;

    /// Returns the duration until the next slot.
    fn duration_to_next_slot(&self) -> Option<Duration>;

    /// Returns the start time of the slot, as a duration since `UNIX_EPOCH`.
    fn start_of(&self, slot: Slot) -> Option<Duration>;

    /// Returns the first slot to be returned at the genesis time.
    fn genesis_slot(&self) -> Slot;

    /// Returns the `Duration` from `UNIX_EPOCH` to the genesis time.
    fn genesis_duration(&self) -> Duration;

    /// Returns the `Duration` since the start of the current `Slot` at seconds
    /// precision. Useful in determining whether to apply proposer boosts.
    fn seconds_from_current_slot_start(&self) -> Option<Duration> {
        self.now_duration()
            .and_then(|now| now.checked_sub(self.genesis_duration()))
            .map(|duration_into_slot| {
                Duration::from_secs(duration_into_slot.as_secs() % self.slot_duration().as_secs())
            })
    }

    /// Returns the `Duration` since the start of the current `Slot` at
    /// milliseconds precision.
    fn millis_from_current_slot_start(&self) -> Option<Duration> {
        self.now_duration()
            .and_then(|now| now.checked_sub(self.genesis_duration()))
            .map(|duration_into_slot| {
                Duration::from_millis(
                    (duration_into_slot.as_millis() % self.slot_duration().as_millis()) as u64
                )
            })
    }

    /// Produces a *new* slot clock with the same configuration of `self`,
    /// except that clock is "frozen" at the `freeze_at` time.
    ///
    /// This is useful for observing the slot clock at arbitrary fixed points in
    /// time.
    fn freeze_at(&mut self, freeze_at: Duration) -> ManualSlotClock {
        let mut slot_clock = ManualSlotClock::new(
            self.genesis_slot(),
            self.genesis_duration(),
            self.slot_duration()
        );
        slot_clock.set_current_time(freeze_at);
        slot_clock
    }
}

/// Determines the present slot based upon the present system time.
#[derive(Clone)]
pub struct SystemTimeSlotClock {
    clock: ManualSlotClock
}

impl SystemTimeSlotClock {
    /// Creates a new slot clock where the first slot is `genesis_slot`, genesis
    /// occurred `genesis_duration` after the `UNIX_EPOCH` and each slot is
    /// `slot_duration` apart.
    fn new(genesis_slot: Slot, genesis_duration: Duration, slot_duration: Duration) -> Self {
        Self { clock: ManualSlotClock::new(genesis_slot, genesis_duration, slot_duration) }
    }
}

impl SlotClock for SystemTimeSlotClock {
    fn new_with_chain_id(chain_id: u64) -> Option<Self> {
        let genesis_slot = Slot(0);
        match chain_id {
            1 => Some(SystemTimeSlotClock::new(
                genesis_slot,
                Duration::from_secs(1606824023),
                Duration::from_secs(12)
            )),
            11155111 => Some(SystemTimeSlotClock::new(
                genesis_slot,
                Duration::from_secs(1655733600),
                Duration::from_secs(12)
            )),
            _ => None
        }
    }

    fn now(&self) -> Option<Slot> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
        self.clock.slot_of(now)
    }

    fn is_prior_to_genesis(&self) -> Option<bool> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
        Some(now < *self.clock.genesis_duration())
    }

    fn now_duration(&self) -> Option<Duration> {
        SystemTime::now().duration_since(UNIX_EPOCH).ok()
    }

    fn slot_of(&self, now: Duration) -> Option<Slot> {
        self.clock.slot_of(now)
    }

    fn duration_to_next_slot(&self) -> Option<Duration> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
        self.clock.duration_to_next_slot_from(now)
    }

    fn slot_duration(&self) -> Duration {
        self.clock.slot_duration()
    }

    fn start_of(&self, slot: Slot) -> Option<Duration> {
        self.clock.start_of(slot)
    }

    fn genesis_slot(&self) -> Slot {
        self.clock.genesis_slot()
    }

    fn genesis_duration(&self) -> Duration {
        *self.clock.genesis_duration()
    }
}

/// Determines the present slot based upon a manually-incremented UNIX
/// timestamp.
pub struct ManualSlotClock {
    genesis_slot:     Slot,
    /// Duration from UNIX epoch to genesis.
    genesis_duration: Duration,
    /// Duration from UNIX epoch to right now.
    current_time:     Duration,
    /// The length of each slot.
    slot_duration:    Duration
}

impl Clone for ManualSlotClock {
    fn clone(&self) -> Self {
        ManualSlotClock {
            genesis_slot:     self.genesis_slot,
            genesis_duration: self.genesis_duration,
            current_time:     self.current_time.clone(),
            slot_duration:    self.slot_duration
        }
    }
}

impl ManualSlotClock {
    /// Creates a new slot clock where the first slot is `genesis_slot`, genesis
    /// occurred `genesis_duration` after the `UNIX_EPOCH` and each slot is
    /// `slot_duration` apart.
    fn new(genesis_slot: Slot, genesis_duration: Duration, slot_duration: Duration) -> Self {
        if slot_duration.as_millis() == 0 {
            panic!("ManualSlotClock cannot have a < 1ms slot duration");
        }

        Self { genesis_slot, current_time: genesis_duration, genesis_duration, slot_duration }
    }

    pub fn set_slot(&mut self, slot: u64) {
        let slots_since_genesis = slot
            .checked_sub(self.genesis_slot.0)
            .expect("slot must be post-genesis")
            .try_into()
            .expect("slot must fit within a u32");
        self.current_time = self.genesis_duration + self.slot_duration * slots_since_genesis;
    }

    pub fn set_current_time(&mut self, duration: Duration) {
        self.current_time = duration;
    }

    pub fn advance_time(&mut self, duration: Duration) {
        let current_time = self.current_time;
        self.current_time = current_time.add(duration);
    }

    pub fn advance_slot(&mut self) {
        self.set_slot(self.now().unwrap().0 + 1)
    }

    pub fn genesis_duration(&self) -> &Duration {
        &self.genesis_duration
    }

    /// Returns the duration from `now` until the start of `slot`.
    ///
    /// Will return `None` if `now` is later than the start of `slot`.
    pub fn duration_to_slot(&self, slot: Slot, now: Duration) -> Option<Duration> {
        self.start_of(slot)?.checked_sub(now)
    }

    /// Returns the duration between `now` and the start of the next slot.
    pub fn duration_to_next_slot_from(&self, now: Duration) -> Option<Duration> {
        if now < self.genesis_duration {
            self.genesis_duration.checked_sub(now)
        } else {
            self.duration_to_slot(Slot(self.slot_of(now)?.0 + 1), now)
        }
    }
}

impl SlotClock for ManualSlotClock {
    fn new_with_chain_id(chain_id: u64) -> Option<Self> {
        let genesis_slot = Slot(0);
        match chain_id {
            1 => Some(ManualSlotClock::new(
                genesis_slot,
                Duration::from_secs(1606824023),
                Duration::from_secs(12)
            )),
            11155111 => Some(ManualSlotClock::new(
                genesis_slot,
                Duration::from_secs(1655733600),
                Duration::from_secs(12)
            )),
            _ => None
        }
    }

    fn now(&self) -> Option<Slot> {
        self.slot_of(self.current_time)
    }

    fn is_prior_to_genesis(&self) -> Option<bool> {
        Some(self.current_time < self.genesis_duration)
    }

    fn now_duration(&self) -> Option<Duration> {
        Some(self.current_time)
    }

    fn slot_of(&self, now: Duration) -> Option<Slot> {
        let genesis = self.genesis_duration;

        if now >= genesis {
            let since_genesis = now
                .checked_sub(genesis)
                .expect("Control flow ensures now is greater than or equal to genesis");
            let slot = Slot((since_genesis.as_millis() / self.slot_duration.as_millis()) as u64);
            Some(Slot(slot.0 + self.genesis_slot.0))
        } else {
            None
        }
    }

    fn duration_to_next_slot(&self) -> Option<Duration> {
        self.duration_to_next_slot_from(self.current_time)
    }

    fn slot_duration(&self) -> Duration {
        self.slot_duration
    }

    /// Returns the duration between UNIX epoch and the start of `slot`.
    fn start_of(&self, slot: Slot) -> Option<Duration> {
        let slot = slot.0.checked_sub(self.genesis_slot.0)?.try_into().ok()?;
        let unadjusted_slot_duration = self.slot_duration.checked_mul(slot)?;

        self.genesis_duration.checked_add(unadjusted_slot_duration)
    }

    fn genesis_slot(&self) -> Slot {
        self.genesis_slot
    }

    fn genesis_duration(&self) -> Duration {
        self.genesis_duration
    }
}
