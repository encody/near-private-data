use cuckoofilter::CuckooFilter;
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LookupMap, Vector},
    env,
    json_types::Base64VecU8,
    near, require, BorshStorageKey, IntoStorageKey, NearToken, PanicOnDefault, PromiseOrValue,
};
use near_sdk_contract_tools::{event, standard::nep297::Event};
use siphasher::sip::SipHasher;

mod filter;
use filter::BorshCuckooFilter;

const AGGREGATOR_CAPACITY: u64 = (1 << 10) - 1;

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Messages,
    CurrentAggregator,
    AggregatorHistory,
}

#[event(
    standard = "x-message-repository",
    version = "1.0.0",
    serde = "near_sdk::serde"
)]
enum ContractEvent {
    Publish { sequence_hash: Base64VecU8 },
}

type Aggregator = BorshCuckooFilter<SipHasher>;

#[near]
pub struct AggregatorRecord {
    pub end_block_timestamp_ms: u64,
    pub aggregator: Aggregator,
}

#[derive(Debug, Clone, PartialEq)]
#[near(serializers = [borsh, json])]
pub struct Message {
    pub message: Base64VecU8,
    pub block_timestamp_ms: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MessageRepository {
    messages: LookupMap<Vec<u8>, Message>,
    aggregator_history: Vector<AggregatorRecord>,
    aggregator_storage_usage: u64,
}

fn new_aggregator() -> Aggregator {
    CuckooFilter::with_capacity(AGGREGATOR_CAPACITY as usize).into()
}

fn get_lazy<T: BorshDeserialize>(key: impl IntoStorageKey) -> Option<T> {
    let bytes = env::storage_read(&key.into_storage_key())?;
    borsh::from_slice(&bytes).ok()
}

fn write<T: BorshSerialize>(key: impl IntoStorageKey, value: T) {
    env::storage_write(&key.into_storage_key(), &borsh::to_vec(&value).unwrap());
}

#[near]
impl MessageRepository {
    #[init]
    pub fn new() -> Self {
        let aggregator_storage_usage = {
            let start_usage = env::storage_usage();
            write(StorageKey::CurrentAggregator, new_aggregator());
            let end_usage = env::storage_usage();
            end_usage - start_usage // should never underflow if everything is working properly
        };

        Self {
            messages: LookupMap::new(StorageKey::Messages),
            aggregator_storage_usage,
            aggregator_history: Vector::new(StorageKey::AggregatorHistory),
        }
    }

    fn add_to_current_aggregator(&mut self, bytes: &[u8]) {
        let mut current_aggregator: Aggregator = get_lazy(StorageKey::CurrentAggregator).unwrap();

        // create new aggregator if current one is full
        if current_aggregator.0.len() as u64 >= AGGREGATOR_CAPACITY {
            let record = AggregatorRecord {
                aggregator: current_aggregator,
                end_block_timestamp_ms: env::block_timestamp_ms(),
            };
            self.aggregator_history.push(&record);
            current_aggregator = new_aggregator();
        }

        current_aggregator.0.add(bytes).unwrap();
        write(StorageKey::CurrentAggregator, current_aggregator);
    }

    pub fn get_message(&self, sequence_hash: Base64VecU8) -> Option<Message> {
        self.messages.get(&sequence_hash.0)
    }

    pub fn get_aggregators_since(&self, block_timestamp_ms: u64) -> Vec<Base64VecU8> {
        let mut history = self
            .aggregator_history
            .iter()
            .rev()
            .map_while(|a| {
                if block_timestamp_ms > a.end_block_timestamp_ms {
                    Some(borsh::to_vec(&a.aggregator).unwrap().into())
                } else {
                    None
                }
            })
            .collect::<Vec<Base64VecU8>>();

        history.push(
            borsh::to_vec(&get_lazy::<Aggregator>(StorageKey::CurrentAggregator).unwrap())
                .unwrap()
                .into(),
        );

        history
    }

    #[payable]
    pub fn publish(
        &mut self,
        sequence_hash: Base64VecU8,
        message: Base64VecU8,
    ) -> PromiseOrValue<()> {
        require!(
            !self.messages.contains_key(&sequence_hash.0),
            "Sequence hash already exists."
        );

        // outside of storage usage calculation so that users aren't charged when a new aggregator is created
        self.add_to_current_aggregator(&sequence_hash.0);

        let item_aggregator_fee = {
            let aggregator_storage_cost =
                env::storage_byte_cost().saturating_mul(self.aggregator_storage_usage as u128);
            let single_item_storage_cost =
                aggregator_storage_cost.saturating_div(AGGREGATOR_CAPACITY as u128);
            let remainder = aggregator_storage_cost.as_yoctonear() % AGGREGATOR_CAPACITY as u128;
            if remainder > 0 {
                single_item_storage_cost.saturating_add(NearToken::from_yoctonear(1))
            } else {
                single_item_storage_cost
            }
        };

        let initial_storage_usage = env::storage_usage();

        self.messages.insert(
            &sequence_hash.0,
            &Message {
                message,
                block_timestamp_ms: env::block_timestamp_ms(),
            },
        );

        ContractEvent::Publish { sequence_hash }.emit();

        near_sdk_contract_tools::utils::apply_storage_fee_and_refund(
            initial_storage_usage,
            item_aggregator_fee.as_yoctonear(),
        )
        .map_or(PromiseOrValue::Value(()), |p| p.into())
    }
}
