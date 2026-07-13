pub mod cache;
pub mod decoder;
pub mod feed;
pub mod hot_path;
mod hot_path_safety;
pub mod noxa_abi;
pub mod noxa_launch;
pub mod noxa_policy;
pub mod noxa_rpc;
pub mod noxa_trade;
pub mod paper;
pub mod probe;
pub mod robinhood;
pub mod rpc;
pub mod sequencer;
pub mod testnet_orchestrator;
pub mod uniswap_v2;
pub mod v2_simulator;
pub mod v3_pool;

pub use cache::{CacheApplyReport, CacheCheckpoint, CacheError, ConfirmedReserveCache};
pub use decoder::{
    Candidate, DecodeError, DecodeReport, FeedDecoder, Filter, TransactionContext,
    TransactionFingerprint,
};
pub use hot_path::{
    HotPathError, HotPathExecutor, HotPathReport, HotPathStrategy, HotTransaction,
    ReconciliationJob, SubmissionResult,
};
pub use noxa_abi::{
    NoxaLaunchEvent, NoxaLaunchHeader, NoxaLaunchIntent, ReceiptLog, V3ExactInputIntent,
    V3ExactOutputIntent, V3PoolCreatedEvent, V3PoolEvent, decode_launch_call, decode_launch_header,
    decode_pool_created, decode_token_launched, decode_v3_exact_input_single,
    decode_v3_exact_output_single, decode_v3_pool_event, encode_v3_exact_input_single,
    encode_v3_exact_output_single,
};
pub use noxa_launch::{HydratedNoxaLaunch, NoxaHydrationError, hydrate_noxa_launch_receipt};
pub use noxa_policy::{
    NoxaPolicyDecision, NoxaPolicyInput, NoxaRejectReason, evaluate_noxa_policy,
};
pub use noxa_rpc::{
    FactoryStatus, NoxaReceipt, NoxaRpcClient, ObservedLaunchLog, RobinhoodBlock,
    RobinhoodTransaction, RpcMetricsSnapshot, TokenRestrictionSnapshot,
};
pub use noxa_trade::{PreparedRawTransaction, TradePlanError, TradeTransactionPlan};
pub use paper::{PaperDecision, PaperPolicy, PaperRejectReason, ReservePaperDecision};
pub use probe::{FrameReport, SequenceObservation, SequenceTracker};
pub use rpc::{FactoryBootstrap, SyncUpdate, V2SnapshotClient};
pub use sequencer::{
    ConditionalOptions, ConditionalResponse, SequencerClient, build_conditional_request,
    classify_conditional_response,
};
pub use testnet_orchestrator::{
    CanaryError, ConditionalRetryDecision, ConditionalRetryState, DedicatedNonceManager,
    NonceError, NonceLease, NonceLeaseState, PreflightError, RiskError, RiskLedger, RiskLimits,
    RiskReservation, RiskStatus, TestnetCanaryPlan, TradePreflightInput, ValidatedTestnetCanary,
    evaluate_testnet_preflight, validate_signed_testnet_canary,
};
pub use uniswap_v2::{V2SwapIntent, V2SwapKind, decode_v2_exact_input};
pub use v2_simulator::{
    HopQuote, OrderedCopyQuote, PairSnapshot, QuoteError, ReserveBook, ReserveCache, get_amount_out,
};
pub use v3_pool::{V3PoolError, V3PoolState, V3Quote};
