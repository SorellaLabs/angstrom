/*
pub fn verifying_staker() -> bool {
    let their_hello_msg = their_hello.clone();

    tracing::trace!(
        hello=?their_hello,
        "validating incoming p2p hello from peer"
    );

    tracing::trace!(?hello.protocol_version, ?their_hello.protocol_version, "validating protocol version");
    if (hello.protocol_version as u8) != their_hello.protocol_version as u8 {
        // send a disconnect message notifying the peer of the protocol version mismatch
        self.send_disconnect(DisconnectReason::IncompatibleP2PProtocolVersion)
            .await?;
        return Err(P2PStreamError::MismatchedProtocolVersion {
            expected: hello.protocol_version as u8,
            got:      their_hello.protocol_version as u8
        })
    }

    tracing::trace!(?hello.capabilities, ?their_hello.capabilities, "validating capabilities version");
    // determine shared capabilities (currently returns only one capability)
    let capability_res =
        set_capability_offsets(hello.capabilities, their_hello.capabilities.clone());

    let shared_capability =
        match capability_res {
            Err(err) => {
                // we don't share any capabilities, send a disconnect message
                self.send_disconnect(DisconnectReason::UselessPeer).await?;
                Err(err)
            }
            Ok(cap) => Ok(cap)
        }?;

    tracing::trace!(?their_hello.signature, "Validating Signature -- DECODING");

    //let hash = keccak256(&their_hello.id);
    //let pub_key_addr = Address::from_slice(&hash[12..]);
    //println!("ADDRESSES: {:#x} --- {:#x}\n\n\n", real_addr, address);

    let their_sig: [u8; 64] = their_hello.signature.try_into().unwrap();
    let rec_id: [u8; 4] = their_hello.sig_recovery_id.try_into().unwrap();
    let their_rec_id = i32::from_be_bytes(rec_id);
    let sig =
        RecoverableSignature::from_compact(&their_sig, RecoveryId::from_i32(their_rec_id).unwrap())
            .unwrap();

    let msg: Message = Message::from_slice(their_hello.signed_hello.as_ref() as &[u8]).unwrap();
    let recovered_pub_key = sig.recover(&msg).unwrap();
    let pub_key: [u8; 64] = recovered_pub_key.serialize_uncompressed()[1..]
        .try_into()
        .unwrap();

    if B512::from_slice(&pub_key) != their_hello.id {
        self.send_disconnect(DisconnectReason::NoRecoveredSigner)
            .await?;
        return Err(P2PStreamError::HandshakeError(P2PHandshakeError::UnableToRecoverSigner(
            format!("Address Mismatch {:#x}, {:#x}", B512::from_slice(&pub_key), their_hello.id)
        )))
    }

    tracing::trace!(
        //?public_key,
        ?valid_stakers,
        "Validating Signature -- CHECKING CURRENT STAKERS STAKERS"
    );
    /*
            if !valid_stakers.contains(&their_hello.id) {
                self.send_disconnect(DisconnectReason::SignerNotStaked)
                    .await?;
                return Err(P2PStreamError::HandshakeError(P2PHandshakeError::SignerNotStaked(
                    their_hello.id.clone(),
                )));
            }
    */
    //tracing::trace!(?signature, ?their_hello.id, "signature and public key
    // valid");
    tracing::trace!("creating P2P stream");
    //let stream = P2PStream::new(self.inner, shared_capability);

    Ok((stream, their_hello_msg))
}
*/
