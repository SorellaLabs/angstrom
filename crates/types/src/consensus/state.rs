use super::{BlockId, GuardSet, Time};

// type State struct {
// 	Version tmstate.Version
//
// 	// immutable
// 	ChainID       string
// 	InitialHeight int64 // should be 1, not 0, when starting from height 1
//
// 	// LastBlockHeight=0 at genesis (ie. block(H=0) does not exist)
// 	LastBlockHeight int64
// 	LastBlockID     types.BlockID
// 	LastBlockTime   time.Time
//
// 	// LastValidators is used to validate block.LastCommit.
// 	// Validators are persisted to the database separately every time they
// change, 	// so we can query for historical validator sets.
// 	// Note that if s.LastBlockHeight causes a valset change,
// 	// we set s.LastHeightValidatorsChanged = s.LastBlockHeight + 1 + 1
// 	// Extra +1 due to nextValSet delay.
// 	NextValidators              *types.ValidatorSet
// 	Validators                  *types.ValidatorSet
// 	LastValidators              *types.ValidatorSet
// 	LastHeightValidatorsChanged int64
//
// 	// Consensus parameters used for validating blocks.
// 	// Changes returned by EndBlock and updated after Commit.
// 	ConsensusParams                  tmproto.ConsensusParams
// 	LastHeightConsensusParamsChanged int64
//
// 	// Merkle root of the results from executing prev block
// 	LastResultsHash []byte
//
// 	// the latest AppHash we've received from calling abci.Commit()
// 	AppHash []byte
// }
pub struct State {
    pub version:         String,
    pub chain_id:        String,
    pub last_height:     u64,
    pub last_id:         BlockId,
    pub last_block_time: Time,

    pub next_guards: GuardSet,
    pub guards: GuardSet,
    pub last_guards: GuardSet,
    pub last_height_validators_changed: u64,

    pub consensus_params:                     (),
    pub last_height_consensus_params_changed: (),
    pub last_results_hash:                    &[u8],

    pub app_hash: &[u8]
}
