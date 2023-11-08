use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

pub struct WallClock {
    boot_time_unix_seconds: Mutex<CriticalSectionRawMutex, u64>,
}

impl WallClock {
    pub const fn new() -> Self {
        Self {
            boot_time_unix_seconds: Mutex::new(1699321495),
        }
    }

    pub async fn get_time(&self) -> u64 {
        let now = embassy_time::Instant::now().as_secs();
        let lock = self.boot_time_unix_seconds.lock().await;
        *lock + now
    }

    pub async fn set_time_from_unix(&self, new_time: u64) {
        let now = embassy_time::Instant::now().as_secs();
        let mut lock = self.boot_time_unix_seconds.lock().await;
        *lock = new_time - now;
    }
}
