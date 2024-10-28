use alloy::{
    primitives::*,
    rlp::{length_of_length, Decodable, Encodable, Error, Header, RlpDecodable, RlpEncodable},
    sol,
    sol_types::{eip712_domain, Eip712Domain}
};
use serde::{Deserialize, Serialize};

sol! {
    #![sol(all_derives = true)]

    interface Angstrom {
        event OwnershipHandoverCanceled(address indexed pendingOwner);
        event OwnershipHandoverRequested(address indexed pendingOwner);
        event OwnershipTransferred(address indexed oldOwner, address indexed newOwner);

        type Currency is address;

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct ExecutedOrder {
            Order order;
            bytes signature;
            uint256 amountInActual;
            uint256 amountOutActual;
        }

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct ExecutedComposableOrder {
            /// @member The original composable order from the user.
            ComposableOrder order;
            /// @member The user's signature over the Order.
            bytes signature;
            /// @member The actual executed input amount.
            uint256 amountInActual;
            /// @member The actual executed output amount.
            uint256 amountOutActual;
        }

        #[derive(Serialize, Deserialize)]
        enum OrderType {
            User,
            Searcher,
            Limit,
        }

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct Order {
            uint256 nonce;
            OrderType orderType;
            address currencyIn;
            address currencyOut;
            uint128 amountIn;
            uint128 amountOutMin;
            uint256 deadline;
            bytes preHook;
            bytes postHook;
        }

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct ComposableOrder {
            /// @member The user provided nonce, can only be used once.
            uint256 nonce;
            /// @member The order's type, can enable partial fills.
            OrderType orderType;
            /// @member The input currency for the order.
            Currency currencyIn;
            /// @member The output currency for the order.
            Currency currencyOut;
            /// @member The (maximum) amount of input currency.
            uint128 amountIn;
            /// @member The minimum amount of output currency.
            uint128 amountOutMin;
            /// @member The order cannot be executed after this timestamp.
            uint256 deadline;
            /// @member An optional user provided hook to run before collecting input
            ///         payment.
            bytes preHook;
            /// @member An optional user provided hook to run after paying the output.
            bytes postHook;
        }

        #[derive(Default, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct Bundle {
            /// @member All executed user orders.
            ExecutedOrder[] orders;
            /// @member All executed composable orders.
            ExecutedComposableOrder[] composableOrders;
            /// @member Abi-encoded UniswapData.
            bytes uniswapData;
        }

        struct SwapParams {
            bool zeroForOne;
            int256 amountSpecified;
            uint160 sqrtPriceLimitX96;
        }

        #[derive(Serialize, Deserialize)]
        struct PoolKey {
            address currency0;
            address currency1;
            uint24 fee;
            int24 tickSpacing;
            address hooks;
        }

        /// @notice Uniswap instructions to execute after lock is taken.
        struct UniswapData {
            /// @member The discrete swaps to perform, there should be at most one entry
            ///         per pool.
            PoolSwap[] swaps;
            /// @member The currency settlements to perform, there should be at most one
            ///         entry per currency.
            CurrencySettlement[] currencies;
            /// @member The fees to pay to each pool, there should be at most one entry
            ///         per pool.
            PoolFees[] pools;
        }

        /// @notice Instruction to execute a swap on UniswapV4.
        struct PoolSwap {
            /// @member The pool to perform the swap on.
            PoolKey pool;
            /// @member The input currency.
            Currency currencyIn;
            /// @member The amount of input.
            uint256 amountIn;
        }

        /// @notice Instruction to settle an amount of currency.
        struct CurrencySettlement {
            /// @member The currency to settle.
            Currency currency;
            /// @member The amount to settle, positive indicates we must pay, negative
            ///         indicates we are paid.
            int256 amountNet;
        }

        /// @notice Instruction to donate revenue to a pool.
        struct PoolFees {
            /// @member The pool to pay fees to.
            PoolKey pool;
            /// @member The amount0 fee.
            uint256 fees0;
            /// @member The amount1 fee.
            uint256 fees1;
        }

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct LowerBound {
            address proposer;  // Address of the proposer, used to verify the sender
            Lvr[] lvr_comp;      // Array of LVR structures representing lower bound constraints
        }

        #[derive(Serialize, Deserialize, RlpEncodable, RlpDecodable)]
        struct Lvr {
            uint256 postArbPrice; // The price post lower bound arbitrage Optimization pending.
            uint256 lvrComp;     // The bribe amount, inclusive of trading fees from the vanilla bundle.
            uint64 proportion;    // The percentage of arbitrage volume bribed to LPs, precomputed for efficiency.
        }


        function beforeSwap(address aSender, SwapParams memory, SwapParams memory, bytes memory)
            external
            view
            returns (bytes4 rSelector);
        function cancelOwnershipHandover() external payable;
        function claimFees(address aCurrency, address aRecipient) external;
        function completeOwnershipHandover(address pendingOwner) external payable;
        function eip712Domain()
            external
            view
            returns (
                bytes1 fields,
                string memory name,
                string memory version,
                uint256 chainId,
                address verifyingContract,
                bytes32 salt,
                uint256[] memory extensions
            );
        function invalidateUnorderedNonces(uint256 wordPos, uint256 mask) external;
        function lockAcquired(bytes memory aBundle) external returns (bytes memory);
        function nonceBitmap(address, uint256) external view returns (uint256);
        function owner() external view returns (address result);
        function ownershipHandoverExpiresAt(
            address pendingOwner
        ) external view returns (uint256 result);
        function poolManager() external view returns (address);
        function process(Bundle memory aBundle) external;
        function renounceOwnership() external payable;
        function requestOwnershipHandover() external payable;
        function transferOwnership(address newOwner) external payable;
    }
}

impl Encodable for Angstrom::PoolKey {
    fn encode(&self, out: &mut dyn bytes::BufMut) {
        Header { list: false, payload_length: 68 }.encode(out);

        self.currency0.encode(out);
        self.currency1.encode(out);
        self.fee.encode(out);

        let tick_spacing: [u8; 3] = self.tickSpacing.to_be_bytes();
        tick_spacing.encode(out);
        self.hooks.encode(out);
    }
}

impl Decodable for Angstrom::PoolKey {
    fn decode(buf: &mut &[u8]) -> alloy::rlp::Result<Self> {
        let _ = Header::decode(buf)?;

        let cur_0 = Address::decode(buf)?;
        let cur_1 = Address::decode(buf)?;
        let fee = Uint::<24, 1>::decode(buf)?;
        let spacing: [u8; 4] = Decodable::decode(buf)?;
        let tick_spacing = Signed::<24, 1>::from_be_bytes(spacing);

        let hooks = Address::decode(buf)?;

        Ok(Self { hooks, fee, tickSpacing: tick_spacing, currency0: cur_0, currency1: cur_1 })
    }
}

impl Encodable for Angstrom::OrderType {
    fn encode(&self, out: &mut dyn bytes::BufMut) {
        let byte: u8 = (*self) as u8;
        out.put_u8(byte)
    }
}
impl Decodable for Angstrom::OrderType {
    fn decode(buf: &mut &[u8]) -> Result<Self, Error> {
        unsafe { std::mem::transmute(u8::decode(buf)) }
    }
}

impl Encodable for Angstrom::CurrencySettlement {
    fn length(&self) -> usize {
        let length = 53;
        length + length_of_length(length)
    }

    fn encode(&self, out: &mut dyn bytes::BufMut) {
        Header { list: true, payload_length: 53 }.encode(out);

        self.currency.encode(out);
        if self.amountNet.is_negative() {
            1_u8.encode(out);
        } else {
            0_u8.encode(out);
        }
        self.amountNet.twos_complement().encode(out);
    }
}
impl Decodable for Angstrom::CurrencySettlement {
    fn decode(buf: &mut &[u8]) -> alloy::rlp::Result<Self> {
        let Header { list, payload_length } = Header::decode(buf)?;

        if !list {
            return Err(Error::UnexpectedString)
        }

        let started_len = buf.len();
        if started_len < payload_length {
            return Err(Error::InputTooShort)
        }

        let currency = Address::decode(buf)?;
        let side: bool = Decodable::decode(buf)?;
        let amount_net = Uint::<256, 4>::decode(buf)?;

        let res = if side {
            Signed::from_raw((!amount_net).overflowing_add(Uint::from(1)).0)
        } else {
            Signed::from_raw(amount_net)
        };

        let consumed = started_len - buf.len();
        if consumed != payload_length {
            return Err(Error::ListLengthMismatch { expected: payload_length, got: consumed })
        }

        Ok(Self { amountNet: res, currency })
    }
}

// The `eip712_domain` macro lets you easily define an EIP-712 domain
// object :)
#[allow(dead_code)]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
   name: "Angstrom",
   version: "1",
);
