use super::{CompositionError, Inventory, Material, positive};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reservoir {
    Interior,
    /// Unresolved condensate, not a claim of an ocean or exposed ice shell.
    CondensedVolatiles,
    /// The entire gaseous/fluid envelope, not just the column above one bar.
    Gas,
}

impl Reservoir {
    const fn index(self) -> usize {
        match self {
            Self::Interior => 0,
            Self::CondensedVolatiles => 1,
            Self::Gas => 2,
        }
    }
}

/// Immutable source budget = retained allocations + escape, per constituent.
/// Atmosphere columns must never be added as a fourth retained reservoir.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    source: Inventory,
    retained: Inventory,
    reservoirs: [Inventory; 3],
    escaped: Inventory,
}

impl Budget {
    /// Supply allocations; derive escape by subtraction so nothing disappears.
    /// No absolute tolerance can hide loss of a trace constituent.
    pub fn new(
        source: Inventory,
        interior: Inventory,
        condensed_volatiles: Inventory,
        gas: Inventory,
    ) -> Result<Self, CompositionError> {
        let retained = interior
            .checked_add(&condensed_volatiles)?
            .checked_add(&gas)?;
        positive("retained planet mass", retained.total_mass_kg())?;
        let mut escaped_parts = Vec::new();
        for material in Material::ALL {
            let initial = source.mass_kg(material);
            let allocated = retained.mass_kg(material);
            if allocated > initial {
                return Err(CompositionError::BudgetExceeded(material));
            }
            escaped_parts.push((material, initial - allocated));
        }
        Ok(Self {
            source,
            retained,
            reservoirs: [interior, condensed_volatiles, gas],
            escaped: Inventory::new(&escaped_parts)?,
        })
    }

    pub fn source(&self) -> &Inventory {
        &self.source
    }
    pub fn retained(&self) -> &Inventory {
        &self.retained
    }
    pub fn escaped(&self) -> &Inventory {
        &self.escaped
    }
    pub fn reservoir(&self, reservoir: Reservoir) -> &Inventory {
        &self.reservoirs[reservoir.index()]
    }

    /// Atomic transfer: failure leaves the budget unchanged. Escape cannot be
    /// silently recaptured. Transfer preserves the authoritative retained total.
    pub fn transfer(
        &mut self,
        from: Reservoir,
        to: Reservoir,
        material: Material,
        mass_kg: f64,
    ) -> Result<(), CompositionError> {
        super::nonnegative("transfer mass", mass_kg)?;
        if from == to {
            return Err(CompositionError::InvalidTransfer);
        }
        let available = self.reservoir(from).mass_kg(material);
        if mass_kg > available {
            return Err(CompositionError::BudgetExceeded(material));
        }
        if mass_kg == 0.0 {
            return Ok(());
        }
        let destination = self.reservoir(to).mass_kg(material);
        let remainder = available - mass_kg;
        let received = destination + mass_kg;
        // Do not manufacture a trace allocation by subtracting less than one ULP.
        if remainder == available || received == destination {
            return Err(CompositionError::TransferPrecision);
        }
        super::nonnegative("transferred allocation", received)?;
        let mut reservoirs = self.reservoirs;
        reservoirs[from.index()].masses_kg[material.index()] = remainder;
        reservoirs[to.index()].masses_kg[material.index()] = received;
        self.reservoirs = reservoirs;
        Ok(())
    }
}
