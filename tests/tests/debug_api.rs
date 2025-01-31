#![allow(clippy::used_underscore_binding)]
#![cfg(feature = "testing")]
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_primitives::Bytes;
use alloy_rlp::Encodable;
use alloy_rpc_types::TransactionInfo;
use alloy_serde::WithOtherFields;
use kakarot_rpc::{
    client::TransactionHashProvider,
    providers::eth_provider::{database::types::transaction::ExtendedTransaction, BlockProvider, ReceiptProvider},
    test_utils::{
        fixtures::{katana, setup},
        katana::Katana,
        rpc::{start_kakarot_rpc_server, RawRpcParamsBuilder},
    },
};
use reth_primitives::{Block, Log, Receipt, ReceiptWithBloom, TransactionSigned, TransactionSignedEcRecovered};
use reth_rpc_types_compat::transaction::from_recovered_with_block_context;
use rstest::*;
use serde_json::Value;

#[rstest]
#[awt]
#[tokio::test(flavor = "multi_thread")]
async fn test_raw_transaction(#[future] katana: Katana, _setup: ()) {
    let (server_addr, server_handle) =
        start_kakarot_rpc_server(&katana).await.expect("Error setting up Kakarot RPC server");

    // First transaction of the database
    let tx = katana.first_transaction().unwrap();
    // Associated block header
    let header = katana.header_by_hash(tx.block_hash.unwrap()).unwrap();

    // EIP1559
    let reqwest_client = reqwest::Client::new();
    let res = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawTransaction").add_param(format!("0x{:064x}", tx.hash)).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response = res.text().await.expect("Failed to get response body");
    let raw: Value = serde_json::from_str(&response).expect("Failed to deserialize response body");
    let rlp_bytes: Option<Bytes> = serde_json::from_value(raw["result"].clone()).expect("Failed to deserialize result");
    assert!(rlp_bytes.is_some());
    // We can decode the RLP bytes to get the transaction and compare it with the original transaction
    let transaction = TransactionSigned::decode_2718(&mut rlp_bytes.unwrap().as_ref()).unwrap();
    let signer = transaction.recover_signer().unwrap();

    let mut transaction = from_recovered_with_block_context(
        TransactionSignedEcRecovered::from_signed_transaction(transaction, signer),
        TransactionInfo {
            hash: Some(tx.hash),
            block_hash: tx.block_hash,
            block_number: tx.block_number,
            base_fee: header.base_fee_per_gas.map(Into::into),
            index: tx.transaction_index,
        },
        &reth_rpc::eth::EthTxBuilder {},
    );

    let res = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("eth_getTransactionByHash").add_param(format!("0x{:064x}", tx.hash)).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response = res.text().await.expect("Failed to get response body");
    let response: Value = serde_json::from_str(&response).expect("Failed to deserialize response body");
    let rpc_transaction: alloy_rpc_types::Transaction =
        serde_json::from_value(response["result"].clone()).expect("Failed to deserialize result");

    // As per https://github.com/paradigmxyz/reth/blob/603e39ab74509e0863fc023461a4c760fb2126d1/crates/rpc/rpc-types-compat/src/transaction/signature.rs#L17
    if rpc_transaction.transaction_type.unwrap() == 0 {
        transaction.signature = Some(alloy_rpc_types::Signature {
            y_parity: rpc_transaction.signature.unwrap().y_parity,
            ..transaction.signature.unwrap()
        });
    }

    // For EIP1559, `gas_price` is recomputed as per https://github.com/paradigmxyz/reth/blob/603e39ab74509e0863fc023461a4c760fb2126d1/crates/rpc/rpc-types-compat/src/transaction/mod.rs#L54
    if rpc_transaction.transaction_type.unwrap() == 2 {
        transaction.gas_price = rpc_transaction.gas_price;
    }

    assert_eq!(transaction, rpc_transaction);

    drop(server_handle);
}

#[rstest]
#[awt]
#[tokio::test(flavor = "multi_thread")]
/// Test for fetching raw transactions by block hash and block number.
async fn test_raw_transactions(#[future] katana: Katana, _setup: ()) {
    // Start the Kakarot RPC server.
    let (server_addr, server_handle) =
        start_kakarot_rpc_server(&katana).await.expect("Error setting up Kakarot RPC server");

    // Get the first transaction from the mock data.
    let tx = &katana.first_transaction().unwrap();

    // Get the block hash from the transaction.
    let block_hash = tx.block_hash.unwrap();
    // Get the block number from the transaction and convert it to a u64.
    let block_number = tx.block_number.unwrap();

    // Fetch raw transactions by block hash.
    let reqwest_client = reqwest::Client::new();
    let res_by_block_hash = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawTransactions").add_param(format!("0x{block_hash:064x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    // Get the response body text from the block hash request.
    let response_by_block_hash = res_by_block_hash.text().await.expect("Failed to get response body");
    // Deserialize the response body into a JSON value.
    let raw_by_block_hash: Value =
        serde_json::from_str(&response_by_block_hash).expect("Failed to deserialize response body");
    // Deserialize the "result" field of the JSON value into a vector of bytes.
    let rlp_bytes_by_block_hash: Vec<Bytes> =
        serde_json::from_value(raw_by_block_hash["result"].clone()).expect("Failed to deserialize result");

    // Fetch raw transactions by block number.
    let res_by_block_number = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawTransactions").add_param(format!("0x{block_number:016x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    // Get the response body text from the block number request.
    let response_by_block_number = res_by_block_number.text().await.expect("Failed to get response body");
    // Deserialize the response body into a JSON value.
    let raw_by_block_number: Value =
        serde_json::from_str(&response_by_block_number).expect("Failed to deserialize response body");
    // Deserialize the "result" field of the JSON value into a vector of bytes.
    let rlp_bytes_by_block_number: Vec<Bytes> =
        serde_json::from_value(raw_by_block_number["result"].clone()).expect("Failed to deserialize result");

    // Assert equality of transactions fetched by block hash and block number.
    assert_eq!(rlp_bytes_by_block_number, rlp_bytes_by_block_hash);

    // Get the Ethereum provider from the Katana instance.
    let eth_provider = katana.eth_provider();

    for (i, actual_tx) in
        eth_provider.block_transactions(Some(block_number.into())).await.unwrap().unwrap().iter().enumerate()
    {
        // Fetch the transaction for the current transaction hash.
        let tx = katana.eth_client.transaction_by_hash(actual_tx.hash).await.unwrap().unwrap();
        let signature = tx.signature.unwrap();

        // Convert the transaction to a primitives transactions and encode it.
        let rlp_bytes = TransactionSigned::from_transaction_and_signature(
            tx.try_into().unwrap(),
            alloy_primitives::Signature::from_rs_and_parity(
                signature.r,
                signature.s,
                signature.y_parity.map_or(false, |v| v.0),
            )
            .expect("Invalid signature"),
        )
        .encoded_2718();

        // Assert the equality of the constructed receipt with the corresponding receipt from both block hash and block number.
        assert_eq!(rlp_bytes_by_block_number[i], rlp_bytes);
        assert_eq!(rlp_bytes_by_block_hash[i], rlp_bytes);
    }

    // Stop the Kakarot RPC server.
    drop(server_handle);
}

#[rstest]
#[awt]
#[tokio::test(flavor = "multi_thread")]
/// Test for fetching raw receipts by block hash and block number.
async fn test_raw_receipts(#[future] katana: Katana, _setup: ()) {
    // Start the Kakarot RPC server.
    let (server_addr, server_handle) =
        start_kakarot_rpc_server(&katana).await.expect("Error setting up Kakarot RPC server");

    // Get the first transaction from the mock data.
    let tx = &katana.first_transaction().unwrap();

    // Get the block hash from the transaction.
    let block_hash = tx.block_hash.unwrap();
    // Get the block number from the transaction and convert it to a u64.
    let block_number = tx.block_number.unwrap();

    // Fetch raw receipts by block hash.
    let reqwest_client = reqwest::Client::new();
    let res_by_block_hash = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawReceipts").add_param(format!("0x{block_hash:064x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    // Get the response body text from the block hash request.
    let response_by_block_hash = res_by_block_hash.text().await.expect("Failed to get response body");
    // Deserialize the response body into a JSON value.
    let raw_by_block_hash: Value =
        serde_json::from_str(&response_by_block_hash).expect("Failed to deserialize response body");
    // Deserialize the "result" field of the JSON value into a vector of bytes.
    let rlp_bytes_by_block_hash: Vec<Bytes> =
        serde_json::from_value(raw_by_block_hash["result"].clone()).expect("Failed to deserialize result");

    // Fetch raw receipts by block number.
    let res_by_block_number = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawReceipts").add_param(format!("0x{block_number:016x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    // Get the response body text from the block number request.
    let response_by_block_number = res_by_block_number.text().await.expect("Failed to get response body");
    // Deserialize the response body into a JSON value.
    let raw_by_block_number: Value =
        serde_json::from_str(&response_by_block_number).expect("Failed to deserialize response body");
    // Deserialize the "result" field of the JSON value into a vector of bytes.
    let rlp_bytes_by_block_number: Vec<Bytes> =
        serde_json::from_value(raw_by_block_number["result"].clone()).expect("Failed to deserialize result");

    // Assert equality of receipts fetched by block hash and block number.
    assert_eq!(rlp_bytes_by_block_number, rlp_bytes_by_block_hash);

    // Get eth provider
    let eth_provider = katana.eth_provider();

    for (i, receipt) in
        eth_provider.block_receipts(Some(block_number.into())).await.unwrap().unwrap().iter().enumerate()
    {
        // Fetch the transaction receipt for the current receipt hash.
        let tx_receipt = eth_provider.transaction_receipt(receipt.transaction_hash).await.unwrap().unwrap();

        // Construct a Receipt instance from the transaction receipt data.
        let r: Bytes = ReceiptWithBloom {
            receipt: Receipt {
                tx_type: Into::<u8>::into(tx_receipt.transaction_type()).try_into().unwrap(),
                success: tx_receipt.inner.status(),
                cumulative_gas_used: TryInto::<u64>::try_into(tx_receipt.inner.inner.cumulative_gas_used()).unwrap(),
                logs: tx_receipt
                    .inner
                    .inner
                    .logs()
                    .iter()
                    .filter_map(|log| Log::new(log.address(), log.topics().to_vec(), log.data().data.clone()))
                    .collect(),
            },
            bloom: *receipt.inner.inner.logs_bloom(),
        }
        .encoded_2718()
        .into();

        // Assert the equality of the constructed receipt with the corresponding receipt from both block hash and block number.
        assert_eq!(rlp_bytes_by_block_number[i], r);
        assert_eq!(rlp_bytes_by_block_hash[i], r);
    }

    // Stop the Kakarot RPC server.
    drop(server_handle);
}

#[rstest]
#[awt]
#[tokio::test(flavor = "multi_thread")]
async fn test_raw_block(#[future] katana: Katana, _setup: ()) {
    let (server_addr, server_handle) =
        start_kakarot_rpc_server(&katana).await.expect("Error setting up Kakarot RPC server");

    // Get the first transaction from the mock data.
    let tx = &katana.first_transaction().unwrap();

    // Get the block number from the transaction and convert it to a u64.
    let block_number = tx.block_number.unwrap();

    let reqwest_client = reqwest::Client::new();
    let res = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawBlock").add_param(format!("0x{block_number:016x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response = res.text().await.expect("Failed to get response body");
    let raw: Value = serde_json::from_str(&response).expect("Failed to deserialize response body");

    let rlp_bytes: Option<Bytes> = serde_json::from_value(raw["result"].clone()).expect("Failed to deserialize result");
    assert!(rlp_bytes.is_some());

    // Query the block with eth_getBlockByNumber
    let res = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(
            RawRpcParamsBuilder::new("eth_getBlockByNumber")
                .add_param(format!("0x{block_number:x}"))
                .add_param(true)
                .build(),
        )
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response = res.text().await.expect("Failed to get response body");
    let response: Value = serde_json::from_str(&response).expect("Failed to deserialize response body");
    let rpc_block: WithOtherFields<alloy_rpc_types::Block<ExtendedTransaction>> =
        serde_json::from_value(response["result"].clone()).expect("Failed to deserialize result");
    let primitive_block = Block::try_from(rpc_block.inner).unwrap();

    // Encode primitive block and compare with the result of debug_getRawBlock
    let mut buf = Vec::new();
    primitive_block.encode(&mut buf);
    assert_eq!(rlp_bytes.clone().unwrap(), Bytes::from(buf));

    // Stop the Kakarot RPC server.
    drop(server_handle);
}

#[rstest]
#[awt]
#[tokio::test(flavor = "multi_thread")]
async fn test_raw_header(#[future] katana: Katana, _setup: ()) {
    // Start Kakarot RPC server and get its address and handle.
    let (server_addr, server_handle) =
        start_kakarot_rpc_server(&katana).await.expect("Error setting up Kakarot RPC server");

    // Get the first transaction from the mock data.
    let tx = &katana.first_transaction().unwrap();

    // Get the block hash from the transaction.
    let block_hash = tx.block_hash.unwrap();
    // Get the block number from the transaction and convert it to a u64.
    let block_number = tx.block_number.unwrap();

    // Create a reqwest client.
    let reqwest_client = reqwest::Client::new();

    // Fetch raw header by block hash.
    let res_by_block_hash = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawHeader").add_param(format!("0x{block_hash:064x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response_by_block_hash = res_by_block_hash.text().await.expect("Failed to get response body");

    // Deserialize response body and extract RLP bytes.
    let raw_by_block_hash: Value =
        serde_json::from_str(&response_by_block_hash).expect("Failed to deserialize response body");
    let rlp_bytes_by_block_hash: Bytes =
        serde_json::from_value(raw_by_block_hash["result"].clone()).expect("Failed to deserialize result");

    // Fetch raw header by block number.
    let res_by_block_number = reqwest_client
        .post(format!("http://localhost:{}", server_addr.port()))
        .header("Content-Type", "application/json")
        .body(RawRpcParamsBuilder::new("debug_getRawHeader").add_param(format!("0x{block_number:016x}")).build())
        .send()
        .await
        .expect("Failed to call Debug RPC");
    let response_by_block_number = res_by_block_number.text().await.expect("Failed to get response body");
    let raw_by_block_number: Value =
        serde_json::from_str(&response_by_block_number).expect("Failed to deserialize response body");
    let rlp_bytes_by_block_number: Bytes =
        serde_json::from_value(raw_by_block_number["result"].clone()).expect("Failed to deserialize result");

    // Assert equality of header fetched by block hash and block number.
    assert_eq!(rlp_bytes_by_block_number, rlp_bytes_by_block_hash);

    // Get eth provider.
    let eth_provider = katana.eth_provider();

    // Fetch the transaction receipt for the current receipt hash.
    let block = eth_provider.block_by_number(block_number.into(), true).await.unwrap().unwrap();

    // Encode header into RLP bytes and assert equality with RLP bytes fetched by block number.
    let mut data = vec![];
    Block::try_from(block.inner).unwrap().header.encode(&mut data);
    assert_eq!(rlp_bytes_by_block_number, data);

    // Stop the Kakarot RPC server.
    drop(server_handle);
}
