use std::time::Duration;
use dashmap::DashMap;
use actix_web::dev::ServiceRequest;
use actix_web::Error as AWError;

use crate::RateLimit;

pub struct MemoryStore
{
    inner: DashMap<String, (usize, Duration)>
}

impl MemoryStore
{
    pub fn new() -> Self {
        MemoryStore {
            inner: DashMap::<String, (usize, Duration)>::new()
        }
    }

    pub fn with_capaticity(capacity: usize) -> Self {
        MemoryStore{
            inner: DashMap::with_capacity(capacity)
        }
    }
}

impl RateLimit for MemoryStore
{
    fn client_identifier(&self, req: &ServiceRequest) -> Result<String, AWError> {
        let soc_addr = req.peer_addr().ok_or(AWError::from(()))?;
        Ok(soc_addr.ip().to_string())
    }

    fn get(&self, key: &str) -> Result<Option<usize>, AWError> {
        if self.inner.contains_key(key){
            let val = self.inner.get(key).unwrap();
            let val = val.value().0;
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    fn set(&self, key: String, val: usize, expiry: Option<Duration>) -> Result<(), AWError> {
        if let Some(c) = expiry{
            // New entry, sets the key
            self.inner.insert(key, (val, c)).unwrap();
        } else {
            // Only update the request count
            let data = self.inner.get(&key).unwrap();
            let data = data.value().1;
            let new_data = (val, Duration::from(data));
            self.inner.insert(key, new_data).unwrap();
        }
        Ok(())
    }

    fn remove(&self, key: String) -> Result<usize, AWError> {
        let val = self.inner.remove::<String>(&key).unwrap();
        let val = val.1;
        Ok(val.0)
    }
}
