//! Thermodynamic estimators derived from SSE expansion-order moments.

/// Online accumulator for the first two moments of expansion order.
///
/// The accumulator stores constant memory regardless of sample count. Use
/// [`ThermodynamicAccumulator::results`] to convert its moments into energy and
/// heat-capacity estimators.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThermodynamicAccumulator {
    samples: u64,
    expansion_order_sum: f64,
    expansion_order_squared_sum: f64,
}

impl ThermodynamicAccumulator {
    /// Records one measured non-identity expansion order.
    pub fn record(&mut self, expansion_order: usize) {
        let order = expansion_order as f64;
        self.samples += 1;
        self.expansion_order_sum += order;
        self.expansion_order_squared_sum += order * order;
    }

    /// Returns the number of recorded measurements.
    #[must_use]
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Converts accumulated moments into thermodynamic estimators.
    ///
    /// The energy convention is `E = energy_shift - <n>/beta`. Heat capacity
    /// is `C = <n^2> - <n>^2 - <n>` in units where Boltzmann's constant is one.
    ///
    /// Returns `None` when no samples have been recorded. Callers must provide
    /// positive `beta` and `num_sites`; these are normally validated by the
    /// sampler and model constructors.
    #[must_use]
    pub fn results(
        &self,
        beta: f64,
        energy_shift: f64,
        num_sites: usize,
    ) -> Option<ThermodynamicResults> {
        if self.samples == 0 {
            return None;
        }
        let count = self.samples as f64;
        let mean_order = self.expansion_order_sum / count;
        let mean_order_squared = self.expansion_order_squared_sum / count;
        let energy = energy_shift - mean_order / beta;
        let heat_capacity = mean_order_squared - mean_order * mean_order - mean_order;

        Some(ThermodynamicResults {
            samples: self.samples,
            mean_expansion_order: mean_order,
            energy,
            energy_per_site: energy / num_sites as f64,
            heat_capacity,
            heat_capacity_per_site: heat_capacity / num_sites as f64,
        })
    }
}

/// Thermodynamic estimates obtained from expansion-order measurements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermodynamicResults {
    /// Number of expansion-order samples.
    pub samples: u64,
    /// Sample mean of the number of non-identity operators.
    pub mean_expansion_order: f64,
    /// Total physical energy including the decomposition shift.
    pub energy: f64,
    /// Physical energy divided by the number of sites.
    pub energy_per_site: f64,
    /// Total heat-capacity estimator in units with `k_B = 1`.
    pub heat_capacity: f64,
    /// Heat-capacity estimator divided by the number of sites.
    pub heat_capacity_per_site: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_expansion_order_moments_to_observables() {
        let mut accumulator = ThermodynamicAccumulator::default();
        accumulator.record(2);
        accumulator.record(4);
        let result = accumulator.results(2.0, 3.0, 2).unwrap();

        assert_eq!(result.samples, 2);
        assert_eq!(result.mean_expansion_order, 3.0);
        assert_eq!(result.energy, 1.5);
        assert_eq!(result.energy_per_site, 0.75);
        assert_eq!(result.heat_capacity, -2.0);
        assert_eq!(result.heat_capacity_per_site, -1.0);
    }
}
