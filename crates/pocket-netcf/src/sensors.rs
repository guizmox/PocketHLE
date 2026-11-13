//! Managed sensor compatibility. The sample is supplied by the emulated device.

use crate::vm::Value;

pub(crate) const UPRIGHT_ACCELERATION: [f32; 3] = [0.0, 9.8, 0.0];

pub struct Sensors {
    acceleration: [f32; 3],
}

impl Default for Sensors {
    fn default() -> Self {
        Self {
            acceleration: UPRIGHT_ACCELERATION,
        }
    }
}

impl Sensors {
    /// Approximate a compass heading from screen-plane tilt.
    pub fn tilt_heading(&self) -> f64 {
        let [x, y, _] = self.acceleration.map(f64::from);
        x.atan2(y).to_degrees().rem_euclid(360.0)
    }

    pub fn set_acceleration(&mut self, mut value: [f32; 3]) {
        if value.iter().all(|axis| axis.is_finite()) {
            // Preserve a defined screen-plane angle when both components are zero.
            if value[0] == 0.0 && value[1] == 0.0 {
                value[1] = f32::EPSILON;
            }
            self.acceleration = value;
        }
    }

    pub fn call(&self, kind: &str, method: &str, args: &[Value]) -> Option<Option<Value>> {
        match (kind, method, args) {
            ("IGSensor" | "IDisposable", "Dispose", [Value::Sensor]) => Some(None),
            ("GSensorFactory", "CreateGSensor", []) => Some(Some(Value::Sensor)),
            ("IGSensor", "get_CurrentValue" | "GetGVector", [Value::Sensor]) => Some(Some(
                Value::Acceleration(self.acceleration.map(f32::to_bits)),
            )),
            ("GVector", "get_X" | "get_Y" | "get_Z", [Value::Acceleration(sample)]) => {
                let index = match method {
                    "get_X" => 0,
                    "get_Y" => 1,
                    _ => 2,
                };
                Some(Some(Value::Float(sample[index])))
            }
            _ => None,
        }
    }
}
