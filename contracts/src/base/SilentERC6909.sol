// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/**
 * @author Adapted from jtriley.eth's version ([source](https://eips.ethereum.org/EIPS/eip-6909)).
 * @author philogy <https://github.com/philogy>
 * @dev ERC6909 with no events
 */
abstract contract SilentERC6909 {
    /// @dev Thrown when owner balance for id is insufficient.
    error InsufficientBalance();

    /// @dev Thrown when spender allowance for id is insufficient.
    error InsufficientPermission();

    /// @notice Owner balance of an id.
    mapping(address owner => mapping(uint256 id => uint256 balance)) public balanceOf;

    /// @notice Spender allowance of an id.
    mapping(address owner => mapping(address spender => mapping(uint256 id => uint256 allowed))) public allowance;

    /// @notice Checks if a spender is approved by an owner as an operator.
    mapping(address owner => mapping(address spender => bool operator)) public isOperator;

    /**
     * @notice Transfers an amount of an id from the caller to a receiver.
     * @param receiver The address of the receiver.
     * @param id The id of the token.
     * @param amount The amount of the token.
     */
    function transfer(address receiver, uint256 id, uint256 amount) public returns (bool) {
        _transfer(msg.sender, receiver, id, amount);
        return true;
    }

    /**
     * @notice Transfers an amount of an id from a sender to a receiver.
     * @param sender The address of the sender.
     * @param receiver The address of the receiver.
     * @param id The id of the token.
     * @param amount The amount of the token.
     */
    function transferFrom(address sender, address receiver, uint256 id, uint256 amount) public returns (bool) {
        _checkAndUseAllowance(msg.sender, sender, id, amount);
        unchecked {
            uint256 fromBal = balanceOf[sender][id];
            if (fromBal < amount) revert InsufficientBalance();
            balanceOf[sender][id] = fromBal - amount;
        }
        balanceOf[receiver][id] += amount;
        return true;
    }

    /**
     * @notice Approves an amount of an id to a spender.
     * @param spender The address of the spender.
     * @param id The id of the token.
     * @param amount The amount of the token.
     */
    function approve(address spender, uint256 id, uint256 amount) public returns (bool) {
        allowance[msg.sender][spender][id] = amount;
        return true;
    }

    /**
     * @notice Sets or removes a spender as an operator for the caller.
     * @param spender The address of the spender.
     * @param approved The approval status.
     */
    function setOperator(address spender, bool approved) public returns (bool) {
        isOperator[msg.sender][spender] = approved;
        return true;
    }

    /**
     * @notice Checks if a contract implements an interface.
     * @param interfaceId The interface identifier, as specified in ERC-165.
     * @return supported True if the contract implements `interfaceId`.
     */
    function supportsInterface(bytes4 interfaceId) public pure virtual returns (bool supported) {
        return interfaceId == 0x0f632fb3 || interfaceId == 0x01ffc9a7;
    }

    /**
     * @dev Checks whether `operator` is auhtorized to transfer `amount` of `id` from `owner`.
     * Consumes any necessary fungible allowance.
     */
    function _checkAndUseAllowance(address operator, address owner, uint256 id, uint256 amount) internal {
        if (isOperator[owner][operator]) return;
        uint256 senderAllowance = allowance[owner][operator][id];
        if (senderAllowance > type(uint248).max) return;
        if (senderAllowance < amount && owner != operator) revert InsufficientPermission();
        unchecked {
            allowance[owner][msg.sender][id] = senderAllowance - amount;
        }
    }

    function _transfer(address from, address to, uint256 id, uint256 amount) internal {
        _burn(from, id, amount);
        _mint(to, id, amount);
    }

    function _mint(address to, uint256 id, uint256 amount) internal {
        balanceOf[to][id] += amount;
    }

    function _burn(address from, uint256 id, uint256 amount) internal {
        uint256 bal = balanceOf[from][id];
        if (bal < amount) revert InsufficientBalance();
        unchecked {
            balanceOf[from][id] = bal - amount;
        }
    }
}
