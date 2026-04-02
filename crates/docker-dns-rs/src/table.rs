use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::net::IpAddr;
use std::sync::Arc;

use color_eyre::eyre::Report;
use hashbrown::HashMap;
use hickory_server::proto::rr::rdata::PTR;
use hickory_server::proto::rr::{LowerName, Name, RData, Record, RecordSet, RecordType, RrKey};
use hickory_server::store::in_memory::InMemoryAuthority;
use ipnet::IpNet;
use tokio::sync::RwLock;
use tracing::{Level, event, instrument};

pub struct AuthorityWrapper {
    forward: Arc<InMemoryAuthority>,
    reverse_zones: RwLock<HashMap<IpNet, Arc<InMemoryAuthority>>>,
}

fn append_to_record_set(
    records: &mut BTreeMap<RrKey, Arc<RecordSet>>,
    key: RrKey,
    owner: Cow<'_, Name>,
    record_type: RecordType,
    rdata: RData,
) {
    match records.entry(key) {
        Entry::Occupied(mut entry) => {
            Arc::make_mut(entry.get_mut()).add_rdata(rdata);
        },
        Entry::Vacant(vacant_entry) => {
            let mut set = RecordSet::with_ttl(owner.into_owned(), record_type, 5);

            set.add_rdata(rdata);

            vacant_entry.insert(Arc::new(set));
        },
    }
}

fn remove_from_ptr_set(
    records: &mut BTreeMap<RrKey, Arc<RecordSet>>,
    ip: IpAddr,
    name: &LowerName,
) {
    let reverse = Name::from(ip);
    let ptr_key = RrKey::new(LowerName::new(&reverse), RecordType::PTR);

    let Entry::Occupied(mut entry) = records.entry(ptr_key) else {
        return;
    };

    let record_to_remove = Record::from_rdata(reverse, 0, RData::PTR(PTR(Name::from(name))));

    let is_empty = {
        let set = Arc::make_mut(entry.get_mut());
        if !set.remove(&record_to_remove, 0) {
            return;
        }
        set.is_empty()
    };

    if is_empty {
        entry.remove();
    }
}

impl AuthorityWrapper {
    pub fn new(forward: Arc<InMemoryAuthority>) -> Self {
        Self {
            forward,
            reverse_zones: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add_reverse_zone(&self, network: IpNet, authority: Arc<InMemoryAuthority>) {
        self.reverse_zones.write().await.insert(network, authority);
    }

    pub async fn remove_reverse_zone(&self, network: &IpNet) {
        self.reverse_zones.write().await.remove(network);
    }

    async fn find_reverse_authority(&self, ip: IpAddr) -> Option<Arc<InMemoryAuthority>> {
        // Docker's IPAM rejects overlapping subnets, so at most one registered
        // reverse zone can contain any given IP. HashMap iteration order is
        // nondeterministic, but that doesn't matter here: there is never more
        // than one match.
        self.reverse_zones
            .read()
            .await
            .iter()
            .find(|&(network, _)| network.contains(&ip))
            .map(|(_, authority)| Arc::clone(authority))
    }

    async fn upsert(&self, name: &Name, address: IpAddr) {
        let rdata: RData = RData::from(address);
        let record_type = rdata.record_type();
        let reverse: Name = address.into();

        {
            let mut lock = self.forward.records_mut().await;

            append_to_record_set(
                &mut lock,
                RrKey::new(LowerName::new(name), record_type),
                Cow::Borrowed(name),
                record_type,
                rdata,
            );
        }

        let Some(reverse_authority) = self.find_reverse_authority(address).await else {
            event!(
                Level::WARN,
                %address,
                "No reverse zone registered for address, PTR record not added"
            );
            return;
        };

        let mut lock = reverse_authority.records_mut().await;

        append_to_record_set(
            &mut lock,
            RrKey::new(LowerName::new(&reverse), RecordType::PTR),
            Cow::Owned(reverse),
            RecordType::PTR,
            RData::PTR(PTR(name.clone())),
        );
    }

    pub async fn add(&self, name: &Name, address: IpAddr) {
        self.upsert(name, address).await;

        event!(Level::INFO, %name, %address);
    }

    #[instrument(skip_all, fields(old_name = %old_key.name, %new_name, r#type = %old_key.record_type))]
    async fn rename_records(&self, old_key: &RrKey, new_name: &Name) -> Result<(), ()> {
        let ips: Vec<IpAddr> = {
            let mut records = self.forward.records_mut().await;

            let Some(record_set) = records.remove(old_key) else {
                return Err(());
            };

            record_set
                .records_without_rrsigs()
                .filter_map(|r| r.data().ip_addr())
                .collect()

            // forward lock released before touching reverse zones
        };

        for ip in ips {
            if let Some(reverse_authority) = self.find_reverse_authority(ip).await {
                remove_from_ptr_set(
                    &mut *reverse_authority.records_mut().await,
                    ip,
                    &old_key.name,
                );
            }

            self.upsert(new_name, ip).await;

            event!(Level::INFO, %ip, "table.rename");
        }

        Ok(())
    }

    pub async fn rename(&self, old_name: &str, new_name: &str) -> Result<(), Report> {
        let new_name_parsed: Name = new_name.parse()?;
        let old_key: LowerName = old_name.strip_prefix('/').unwrap_or(old_name).parse()?;

        let old_key_a = RrKey::new(old_key.clone(), RecordType::A);
        let a_result = self.rename_records(&old_key_a, &new_name_parsed).await;

        let old_key_aaaa = RrKey::new(old_key, RecordType::AAAA);
        let aaaa_result = self.rename_records(&old_key_aaaa, &new_name_parsed).await;

        if let (Err(()), Err(())) = (a_result, aaaa_result) {
            event!(
                Level::WARN,
                %old_name,
                %new_name,
                "No A or AAAA records found"
            );
        }

        Ok(())
    }

    async fn remove_record(&self, name: &Name, ip: IpAddr) -> Result<(), ()> {
        let record_type = RData::from(ip).record_type();
        let key = RrKey::new(LowerName::new(name), record_type);

        {
            let mut records = self.forward.records_mut().await;
            let Entry::Occupied(mut entry) = records.entry(key) else {
                return Err(());
            };

            let record_to_remove = Record::from_rdata(name.clone(), 0, RData::from(ip));

            let is_empty = {
                let set = Arc::make_mut(entry.get_mut());
                if !set.remove(&record_to_remove, 0) {
                    return Err(());
                }
                set.is_empty()
            };

            if is_empty {
                entry.remove();
            }
        }

        if let Some(reverse_authority) = self.find_reverse_authority(ip).await {
            remove_from_ptr_set(
                &mut *reverse_authority.records_mut().await,
                ip,
                &LowerName::new(name),
            );
        }

        event!(Level::INFO, %name, %ip, "table.remove");

        Ok(())
    }

    pub async fn remove_address(&self, name: &Name, ip: IpAddr) {
        if self.remove_record(name, ip).await.is_err() {
            event!(Level::WARN, %name, %ip, "No record found to remove");
        }
    }
}
