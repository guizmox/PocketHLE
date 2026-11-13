//! Managed sensor compatibility. The sample is supplied by the emulated device.

use crate::vm::Value;

pub struct Sensors {
    acceleration: [f32; 3],
}

impl Default for Sensors {
    fn default() -> Self {
        Self {
            acceleration: [0.0, 0.0, 9.8],
        }
    }
}

impl Sensors {
    pub fn set_acceleration(&mut self, value: [f32; 3]) {
        if value.iter().all(|axis| axis.is_finite()) {
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
