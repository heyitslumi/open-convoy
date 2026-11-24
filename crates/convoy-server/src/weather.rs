use convoy_proto::rpc::WeatherRpc;

/// Default sun/rain cycle duration in seconds (1 hour).
const DEFAULT_CYCLE_DURATION_SECS: f32 = 3600.0;

/// Synchronizes weather and daylight cycles across all connected convoy clients.
pub struct WeatherManager {
    /// Rain factor clamped in [0.0, 1.0].
    rain_factor: f32,
    /// Elapsed seconds into current weather cycle.
    elapsed_secs: f32,
    /// Target duration of current weather cycle.
    cycle_duration_secs: f32,
    /// Monotonic cycle counter.
    cycle_count: u32,
}

impl WeatherManager {
    pub fn new() -> Self {
        Self {
            rain_factor: 0.0,
            elapsed_secs: 0.0,
            cycle_duration_secs: DEFAULT_CYCLE_DURATION_SECS,
            cycle_count: 0,
        }
    }

    /// Update rain factor and return reinit RPC.
    pub fn set_rain_factor(&mut self, factor: f32) -> WeatherRpc {
        self.rain_factor = factor.clamp(0.0, 1.0);
        WeatherRpc::ReinitRemoteCycles {
            t1: self.cycle_count,
            t2: self.elapsed_secs as u32,
            t3: self.cycle_duration_secs as u32,
            rain_factor: self.rain_factor,
        }
    }

    /// Step weather timer. When cycle expires, update to next cycle.
    pub fn step(&mut self, delta_secs: f32) -> Option<WeatherRpc> {
        self.elapsed_secs += delta_secs;
        if self.elapsed_secs >= self.cycle_duration_secs {
            self.elapsed_secs -= self.cycle_duration_secs;
            self.cycle_count = self.cycle_count.wrapping_add(1);
            Some(WeatherRpc::UpdateNextCycle(self.cycle_count))
        } else {
            None
        }
    }

    /// Current rain factor in [0.0, 1.0].
    pub fn rain_factor(&self) -> f32 {
        self.rain_factor
    }
}

impl Default for WeatherManager {
    fn default() -> Self {
        Self::new()
    }
}
