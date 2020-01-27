use dashmap::DashMap;
use actix_web::dev::ServiceRequest;
use actix_web::Error as AWError;

use crate::RateLimit;

pub struct MemoryStore
{
    inner: DashMap<String, usize>
}

impl MemoryStore
{
    pub fn new() -> Self {
        MemoryStore {
            inner: DashMap::<String, usize>::new()
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

    fn get(&self, key: &str) -> Result<usize, AWError> {
        let val = self.inner.get(key).unwrap();
        Ok(*val.value())
    }

    fn set(&self, key: String, val: usize) -> Result<(), AWError> {
        self.inner.insert(key, val).unwrap();
        Ok(())
    }

    fn remove(&self, key: String) -> Result<usize, AWError> {
        let val = self.inner.remove::<String>(&key).unwrap();
        Ok(val.1)
    }
}
