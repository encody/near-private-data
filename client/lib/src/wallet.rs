use anyhow::bail;
use near_crypto::Signer;
use near_jsonrpc_client::{methods, AsUrl, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    hash::CryptoHash,
    transaction::{Action, Transaction, TransactionV0},
    types::{AccountId, BlockReference, Finality},
    views::{AccessKeyView, FinalExecutionOutcomeView, QueryRequest},
};
use serde::de::DeserializeOwned;

pub const ONE_TERAGAS: u64 = 10u64.pow(12);
pub const ONE_NEAR: u128 = 10u128.pow(24);

#[derive(Debug)]
pub struct RpcClientWrapper {
    client: JsonRpcClient,
}

impl RpcClientWrapper {
    pub fn new(client: JsonRpcClient) -> Self {
        Self { client }
    }

    pub async fn sync_account_key(
        &self,
        account_id: AccountId,
        public_key: near_crypto::PublicKey,
    ) -> anyhow::Result<(u64, CryptoHash)> {
        let response = self
            .client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: QueryRequest::ViewAccessKey {
                    account_id,
                    public_key,
                },
            })
            .await?;

        match response.kind {
            QueryResponseKind::AccessKey(AccessKeyView { nonce, .. }) => {
                Ok((nonce, response.block_hash))
            }
            _ => bail!("Invalid response from RPC"),
        }
    }

    pub async fn send<M>(&self, method: M) -> MethodCallResult<M::Response, M::Error>
    where
        M: methods::RpcMethod,
    {
        self.client.call(method).await
    }
}

#[derive(Debug)]
pub struct Wallet {
    rpc: RpcClientWrapper,
    pub account_id: AccountId,
    signer: Signer,
}

impl Wallet {
    pub fn new(client: impl AsUrl, account_id: AccountId, signer: Signer) -> Self {
        Self {
            rpc: RpcClientWrapper::new(JsonRpcClient::connect(client)),
            account_id,
            signer,
        }
    }

    pub async fn transact(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> anyhow::Result<FinalExecutionOutcomeView> {
        let (current_nonce, block_hash) = self
            .rpc
            .sync_account_key(self.account_id.clone(), self.signer.public_key())
            .await?;

        let nonce = current_nonce + 1;

        let transaction = Transaction::V0(TransactionV0 {
            nonce,
            block_hash,
            public_key: self.signer.public_key(),
            signer_id: self.account_id.clone(),
            receiver_id,
            actions,
        });

        let signed_transaction = transaction.sign(&self.signer);

        let result = self
            .rpc
            .send(methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest { signed_transaction })
            .await?;

        Ok(result)
    }

    pub async fn view<T: DeserializeOwned>(
        &self,
        account_id: AccountId,
        method_name: impl ToString,
        args: impl ToString,
    ) -> anyhow::Result<T> {
        let response = self
            .rpc
            .send(methods::query::RpcQueryRequest {
                block_reference: BlockReference::Finality(Finality::Final),
                request: QueryRequest::CallFunction {
                    account_id,
                    method_name: method_name.to_string(),
                    args: args.to_string().into_bytes().into(),
                },
            })
            .await?;

        let response = match response.kind {
            QueryResponseKind::CallResult(r) => r,
            _ => bail!("Wrong response: {response:?}"),
        };

        let response: T = serde_json::from_slice(&response.result)?;

        Ok(response)
    }
}
