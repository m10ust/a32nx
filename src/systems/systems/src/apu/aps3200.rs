use super::{ApuGenerator, Turbine, TurbineController, TurbineState};
use crate::{
    electrical::{
        ElectricalStateWriter, Potential, PotentialSource, ProvideFrequency, ProvideLoad,
        ProvidePotential,
    },
    shared::{calculate_towards_target_temperature, random_number, TimedRandom},
    simulation::{SimulationElement, SimulatorWriter, UpdateContext},
};
use std::time::Duration;
use uom::si::{
    electric_current::ampere, electric_potential::volt, f64::*, frequency::hertz, ratio::percent,
    temperature_interval, thermodynamic_temperature::degree_celsius,
};

pub struct ShutdownAps3200Turbine {
    egt: ThermodynamicTemperature,
}
impl ShutdownAps3200Turbine {
    pub fn new() -> Self {
        ShutdownAps3200Turbine {
            egt: ThermodynamicTemperature::new::<degree_celsius>(0.),
        }
    }

    fn new_with_egt(egt: ThermodynamicTemperature) -> Self {
        ShutdownAps3200Turbine { egt }
    }
}
impl Turbine for ShutdownAps3200Turbine {
    fn update(
        mut self: Box<Self>,
        context: &UpdateContext,
        _: bool,
        _: bool,
        controller: &dyn TurbineController,
    ) -> Box<dyn Turbine> {
        self.egt = calculate_towards_ambient_egt(self.egt, context);

        if controller.should_start() {
            Box::new(Starting::new(self.egt))
        } else {
            self
        }
    }

    fn n(&self) -> Ratio {
        Ratio::new::<percent>(0.)
    }

    fn egt(&self) -> ThermodynamicTemperature {
        self.egt
    }

    fn state(&self) -> TurbineState {
        TurbineState::Shutdown
    }
}

struct Starting {
    since: Duration,
    n: Ratio,
    egt: ThermodynamicTemperature,
    ignore_calculated_egt: bool,
}
impl Starting {
    fn new(egt: ThermodynamicTemperature) -> Starting {
        Starting {
            since: Duration::from_secs(0),
            n: Ratio::new::<percent>(0.),
            egt,
            ignore_calculated_egt: true,
        }
    }

    fn calculate_egt(&mut self, context: &UpdateContext) -> ThermodynamicTemperature {
        // Refer to APS3200.md for details on the values below and source data.
        const APU_N_TEMP_CONST: f64 = -92.3417137705543;
        const APU_N_TEMP_X: f64 = -14.36417426895237;
        const APU_N_TEMP_X2: f64 = 12.210567963472547;
        const APU_N_TEMP_X3: f64 = -3.005504263233662;
        const APU_N_TEMP_X4: f64 = 0.3808066398934025;
        const APU_N_TEMP_X5: f64 = -0.02679731462093699;
        const APU_N_TEMP_X6: f64 = 0.001163901295794232;
        const APU_N_TEMP_X7: f64 = -0.0000332668380497951;
        const APU_N_TEMP_X8: f64 = 0.00000064601180727581;
        const APU_N_TEMP_X9: f64 = -0.00000000859285727074;
        const APU_N_TEMP_X10: f64 = 0.00000000007717119413;
        const APU_N_TEMP_X11: f64 = -0.00000000000044761099;
        const APU_N_TEMP_X12: f64 = 0.00000000000000151429;
        const APU_N_TEMP_X13: f64 = -0.00000000000000000227;

        let n = self.n.get::<percent>();

        let temperature = ThermodynamicTemperature::new::<degree_celsius>(
            APU_N_TEMP_CONST
                + (APU_N_TEMP_X * n)
                + (APU_N_TEMP_X2 * n.powi(2))
                + (APU_N_TEMP_X3 * n.powi(3))
                + (APU_N_TEMP_X4 * n.powi(4))
                + (APU_N_TEMP_X5 * n.powi(5))
                + (APU_N_TEMP_X6 * n.powi(6))
                + (APU_N_TEMP_X7 * n.powi(7))
                + (APU_N_TEMP_X8 * n.powi(8))
                + (APU_N_TEMP_X9 * n.powi(9))
                + (APU_N_TEMP_X10 * n.powi(10))
                + (APU_N_TEMP_X11 * n.powi(11))
                + (APU_N_TEMP_X12 * n.powi(12))
                + (APU_N_TEMP_X13 * n.powi(13)),
        );

        // The above calculated EGT can be lower than the ambient temperature,
        // or the current APU EGT (when cooling down). To prevent sudden changes
        // in temperature, we ignore the calculated EGT until it exceeds the current
        // EGT.
        let towards_ambient_egt = calculate_towards_ambient_egt(self.egt, context);
        if temperature > towards_ambient_egt {
            self.ignore_calculated_egt = false;
        }

        if self.ignore_calculated_egt {
            towards_ambient_egt
        } else {
            temperature
        }
    }

    fn calculate_n(&self) -> Ratio {
        const APU_N_CONST: f64 = -0.08013606018640967;
        const APU_N_X: f64 = 2.129832736394534;
        const APU_N_X2: f64 = 3.928273438786404;
        const APU_N_X3: f64 = -1.88613299921213;
        const APU_N_X4: f64 = 0.42749452749180916;
        const APU_N_X5: f64 = -0.05757707967690426;
        const APU_N_X6: f64 = 0.005022142795451004;
        const APU_N_X7: f64 = -0.00029612873626050866;
        const APU_N_X8: f64 = 0.00001204152497871946;
        const APU_N_X9: f64 = -0.00000033829604438116;
        const APU_N_X10: f64 = 0.00000000645140818528;
        const APU_N_X11: f64 = -0.00000000007974743535;
        const APU_N_X12: f64 = 0.00000000000057654695;
        const APU_N_X13: f64 = -0.00000000000000185126;

        // Protect against the formula returning decreasing results after this value.
        const TIME_LIMIT: f64 = 45.12;
        const START_IGNITION_AFTER_SECONDS: f64 = 1.5;
        let ignition_turned_on_secs =
            (self.since.as_secs_f64() - START_IGNITION_AFTER_SECONDS).min(TIME_LIMIT);

        if ignition_turned_on_secs > 0. {
            let n = (APU_N_CONST
                + (APU_N_X * ignition_turned_on_secs)
                + (APU_N_X2 * ignition_turned_on_secs.powi(2))
                + (APU_N_X3 * ignition_turned_on_secs.powi(3))
                + (APU_N_X4 * ignition_turned_on_secs.powi(4))
                + (APU_N_X5 * ignition_turned_on_secs.powi(5))
                + (APU_N_X6 * ignition_turned_on_secs.powi(6))
                + (APU_N_X7 * ignition_turned_on_secs.powi(7))
                + (APU_N_X8 * ignition_turned_on_secs.powi(8))
                + (APU_N_X9 * ignition_turned_on_secs.powi(9))
                + (APU_N_X10 * ignition_turned_on_secs.powi(10))
                + (APU_N_X11 * ignition_turned_on_secs.powi(11))
                + (APU_N_X12 * ignition_turned_on_secs.powi(12))
                + (APU_N_X13 * ignition_turned_on_secs.powi(13)))
            .min(100.)
            .max(0.);

            Ratio::new::<percent>(n)
        } else {
            Ratio::new::<percent>(0.)
        }
    }
}
impl Turbine for Starting {
    fn update(
        mut self: Box<Self>,
        context: &UpdateContext,
        _: bool,
        _: bool,
        controller: &dyn TurbineController,
    ) -> Box<dyn Turbine> {
        self.since += context.delta;
        self.n = self.calculate_n();
        self.egt = self.calculate_egt(context);

        if controller.should_stop() {
            Box::new(Stopping::new(self.egt, self.n))
        } else if (self.n.get::<percent>() - 100.).abs() < f64::EPSILON {
            Box::new(Running::new(self.egt))
        } else {
            self
        }
    }

    fn n(&self) -> Ratio {
        self.n
    }

    fn egt(&self) -> ThermodynamicTemperature {
        self.egt
    }

    fn state(&self) -> TurbineState {
        TurbineState::Starting
    }
}

struct BleedAirUsageEgtDelta {
    current: f64,
    target: f64,
    max: f64,
    min: f64,
}
impl BleedAirUsageEgtDelta {
    fn new() -> Self {
        let randomisation = 0.95 + ((random_number() % 101) as f64 / 1000.);

        Self {
            current: 0.,
            target: 0.,
            max: 90. * randomisation,
            min: 0.,
        }
    }

    fn update(&mut self, context: &UpdateContext, apu_bleed_is_used: bool) {
        self.target = if apu_bleed_is_used {
            self.max
        } else {
            self.min
        };

        if (self.current - self.target).abs() > f64::EPSILON {
            if self.current > self.target {
                self.current -= self.delta_per_second() * context.delta.as_secs_f64();
            } else {
                self.current += self.delta_per_second() * context.delta.as_secs_f64();
            }
        }

        self.current = self.current.max(self.min).min(self.max);
    }

    fn egt_delta(&self) -> TemperatureInterval {
        TemperatureInterval::new::<temperature_interval::degree_celsius>(self.current)
    }

    fn delta_per_second(&self) -> f64 {
        // Loosely based on bleed on data provided in a video by Komp.
        // The very much relates to pneumatics and thus could be improved further
        // once we built that.
        const BLEED_AIR_DELTA_TEMP_CONST: f64 = 0.46763348242588143;
        const BLEED_AIR_DELTA_TEMP_X: f64 = 0.43114440400626697;
        const BLEED_AIR_DELTA_TEMP_X2: f64 = -0.11064487957454393;
        const BLEED_AIR_DELTA_TEMP_X3: f64 = 0.010414691679270397;
        const BLEED_AIR_DELTA_TEMP_X4: f64 = -0.00045307219981909655;
        const BLEED_AIR_DELTA_TEMP_X5: f64 = 0.00001063664878607912;
        const BLEED_AIR_DELTA_TEMP_X6: f64 = -0.00000013763963889674;
        const BLEED_AIR_DELTA_TEMP_X7: f64 = 0.00000000091837058563;
        const BLEED_AIR_DELTA_TEMP_X8: f64 = -0.00000000000246054885;

        let difference = if self.current > self.target {
            self.current - self.target
        } else {
            self.target - self.current
        };

        BLEED_AIR_DELTA_TEMP_CONST
            + (BLEED_AIR_DELTA_TEMP_X * difference)
            + (BLEED_AIR_DELTA_TEMP_X2 * difference.powi(2))
            + (BLEED_AIR_DELTA_TEMP_X3 * difference.powi(3))
            + (BLEED_AIR_DELTA_TEMP_X4 * difference.powi(4))
            + (BLEED_AIR_DELTA_TEMP_X5 * difference.powi(5))
            + (BLEED_AIR_DELTA_TEMP_X6 * difference.powi(6))
            + (BLEED_AIR_DELTA_TEMP_X7 * difference.powi(7))
            + (BLEED_AIR_DELTA_TEMP_X8 * difference.powi(8))
    }
}

struct ApuGenUsageEgtDelta {
    time: Duration,
    base_egt_delta_per_second: f64,
}
impl ApuGenUsageEgtDelta {
    // We just assume it takes 10 seconds to get to our target.
    const SECONDS_TO_REACH_TARGET: u64 = 10;
    fn new() -> Self {
        Self {
            time: Duration::from_secs(0),
            base_egt_delta_per_second: (10. + ((random_number() % 6) as f64))
                / ApuGenUsageEgtDelta::SECONDS_TO_REACH_TARGET as f64,
        }
    }

    fn update(&mut self, context: &UpdateContext, apu_gen_is_used: bool) {
        self.time = if apu_gen_is_used {
            (self.time + context.delta).min(Duration::from_secs(
                ApuGenUsageEgtDelta::SECONDS_TO_REACH_TARGET,
            ))
        } else {
            Duration::from_secs_f64((self.time.as_secs_f64() - context.delta.as_secs_f64()).max(0.))
        };
    }

    fn egt_delta(&self) -> TemperatureInterval {
        TemperatureInterval::new::<temperature_interval::degree_celsius>(
            self.time.as_secs_f64() * self.base_egt_delta_per_second,
        )
    }
}

struct Running {
    egt: ThermodynamicTemperature,
    base_egt: ThermodynamicTemperature,
    base_egt_deviation: TemperatureInterval,
    bleed_air_usage: BleedAirUsageEgtDelta,
    apu_gen_usage: ApuGenUsageEgtDelta,
}
impl Running {
    fn new(egt: ThermodynamicTemperature) -> Running {
        let base_egt = 340. + ((random_number() % 11) as f64);
        Running {
            egt,
            base_egt: ThermodynamicTemperature::new::<degree_celsius>(base_egt),
            // This contains the deviation from the base EGT at the moment of entering the running state.
            // This code assumes the base EGT is lower than the EGT at this point in time, which is always the case.
            // Should this change in the future, then changes have to be made here.
            base_egt_deviation: TemperatureInterval::new::<temperature_interval::degree_celsius>(
                egt.get::<degree_celsius>() - base_egt,
            ),
            bleed_air_usage: BleedAirUsageEgtDelta::new(),
            apu_gen_usage: ApuGenUsageEgtDelta::new(),
        }
    }

    fn calculate_egt(
        &mut self,
        context: &UpdateContext,
        apu_gen_is_used: bool,
        apu_bleed_is_used: bool,
    ) -> ThermodynamicTemperature {
        // Reduce the deviation by 1 per second to slowly creep back to normal temperatures
        self.base_egt_deviation -= TemperatureInterval::new::<temperature_interval::degree_celsius>(
            (context.delta.as_secs_f64() * 1.).min(
                self.base_egt_deviation
                    .get::<temperature_interval::degree_celsius>(),
            ),
        );

        let mut target = self.base_egt + self.base_egt_deviation;
        self.apu_gen_usage.update(context, apu_gen_is_used);
        target += self.apu_gen_usage.egt_delta();

        self.bleed_air_usage.update(context, apu_bleed_is_used);
        target += self.bleed_air_usage.egt_delta();

        target
    }
}
impl Turbine for Running {
    fn update(
        mut self: Box<Self>,
        context: &UpdateContext,
        apu_bleed_is_used: bool,
        apu_gen_is_used: bool,
        controller: &dyn TurbineController,
    ) -> Box<dyn Turbine> {
        self.egt = self.calculate_egt(context, apu_gen_is_used, apu_bleed_is_used);

        if controller.should_stop() {
            Box::new(Stopping::new(self.egt, Ratio::new::<percent>(100.)))
        } else {
            self
        }
    }

    fn n(&self) -> Ratio {
        Ratio::new::<percent>(100.)
    }

    fn egt(&self) -> ThermodynamicTemperature {
        self.egt
    }

    fn state(&self) -> TurbineState {
        TurbineState::Running
    }
}

struct Stopping {
    since: Duration,
    base_temperature: ThermodynamicTemperature,
    n: Ratio,
    egt: ThermodynamicTemperature,
}
impl Stopping {
    fn new(egt: ThermodynamicTemperature, n: Ratio) -> Stopping {
        Stopping {
            since: Duration::from_secs(0),
            base_temperature: egt,
            n,
            egt,
        }
    }

    fn calculate_egt_delta(&self) -> TemperatureInterval {
        // Refer to APS3200.md for details on the values below and source data.
        const APU_N_TEMP_DELTA_CONST: f64 = -125.73137672208446;
        const APU_N_TEMP_DELTA_X: f64 = 2.7141683591219037;
        const APU_N_TEMP_DELTA_X2: f64 = -0.8102923071483102;
        const APU_N_TEMP_DELTA_X3: f64 = 0.08890509495240731;
        const APU_N_TEMP_DELTA_X4: f64 = -0.003509532681984154;
        const APU_N_TEMP_DELTA_X5: f64 = -0.00002709133732344767;
        const APU_N_TEMP_DELTA_X6: f64 = 0.00000749250123766767;
        const APU_N_TEMP_DELTA_X7: f64 = -0.00000030306978045244;
        const APU_N_TEMP_DELTA_X8: f64 = 0.00000000641099706269;
        const APU_N_TEMP_DELTA_X9: f64 = -0.00000000008068326110;
        const APU_N_TEMP_DELTA_X10: f64 = 0.00000000000060754088;
        const APU_N_TEMP_DELTA_X11: f64 = -0.00000000000000253354;
        const APU_N_TEMP_DELTA_X12: f64 = 0.00000000000000000451;

        let n = self.n.get::<percent>();
        TemperatureInterval::new::<temperature_interval::degree_celsius>(
            APU_N_TEMP_DELTA_CONST
                + (APU_N_TEMP_DELTA_X * n)
                + (APU_N_TEMP_DELTA_X2 * n.powi(2))
                + (APU_N_TEMP_DELTA_X3 * n.powi(3))
                + (APU_N_TEMP_DELTA_X4 * n.powi(4))
                + (APU_N_TEMP_DELTA_X5 * n.powi(5))
                + (APU_N_TEMP_DELTA_X6 * n.powi(6))
                + (APU_N_TEMP_DELTA_X7 * n.powi(7))
                + (APU_N_TEMP_DELTA_X8 * n.powi(8))
                + (APU_N_TEMP_DELTA_X9 * n.powi(9))
                + (APU_N_TEMP_DELTA_X10 * n.powi(10))
                + (APU_N_TEMP_DELTA_X11 * n.powi(11))
                + (APU_N_TEMP_DELTA_X12 * n.powi(12)),
        )
    }

    fn calculate_n(&self) -> Ratio {
        // Refer to APS3200.md for details on the values below and source data.
        const APU_N_CONST: f64 = 100.22975364965701;
        const APU_N_X: f64 = -24.692008355859773;
        const APU_N_X2: f64 = 2.6116524551318787;
        const APU_N_X3: f64 = 0.006812541903222142;
        const APU_N_X4: f64 = -0.03134644787752123;
        const APU_N_X5: f64 = 0.0036345606954833213;
        const APU_N_X6: f64 = -0.00021794252200618456;
        const APU_N_X7: f64 = 0.00000798097055109138;
        const APU_N_X8: f64 = -0.00000018481154462604;
        const APU_N_X9: f64 = 0.00000000264691628669;
        const APU_N_X10: f64 = -0.00000000002143677577;
        const APU_N_X11: f64 = 0.00000000000007515448;

        // Protect against the formula returning increasing results after this value.
        const TIME_LIMIT: f64 = 49.411;
        let since = self.since.as_secs_f64().min(TIME_LIMIT);

        let n = (APU_N_CONST
            + (APU_N_X * since)
            + (APU_N_X2 * since.powi(2))
            + (APU_N_X3 * since.powi(3))
            + (APU_N_X4 * since.powi(4))
            + (APU_N_X5 * since.powi(5))
            + (APU_N_X6 * since.powi(6))
            + (APU_N_X7 * since.powi(7))
            + (APU_N_X8 * since.powi(8))
            + (APU_N_X9 * since.powi(9))
            + (APU_N_X10 * since.powi(10))
            + (APU_N_X11 * since.powi(11)))
        .min(100.)
        .max(0.);

        Ratio::new::<percent>(n)
    }
}
impl Turbine for Stopping {
    fn update(
        mut self: Box<Self>,
        context: &UpdateContext,
        _: bool,
        _: bool,
        _: &dyn TurbineController,
    ) -> Box<dyn Turbine> {
        self.since += context.delta;
        self.n = self.calculate_n();
        self.egt = self.base_temperature + self.calculate_egt_delta();

        if self.n.get::<percent>() == 0. {
            Box::new(ShutdownAps3200Turbine::new_with_egt(self.egt))
        } else {
            self
        }
    }

    fn n(&self) -> Ratio {
        self.n
    }

    fn egt(&self) -> ThermodynamicTemperature {
        self.egt
    }

    fn state(&self) -> TurbineState {
        TurbineState::Stopping
    }
}

fn calculate_towards_ambient_egt(
    current_egt: ThermodynamicTemperature,
    context: &UpdateContext,
) -> ThermodynamicTemperature {
    const APU_AMBIENT_COEFFICIENT: f64 = 1.;
    calculate_towards_target_temperature(
        current_egt,
        context.ambient_temperature,
        APU_AMBIENT_COEFFICIENT,
        context.delta,
    )
}

/// APS3200 APU Generator
pub struct Aps3200ApuGenerator {
    number: usize,
    writer: ElectricalStateWriter,
    output: Potential,
    random_voltage: TimedRandom<f64>,
    current: ElectricCurrent,
    potential: ElectricPotential,
    frequency: Frequency,
}
impl Aps3200ApuGenerator {
    const APU_GEN_POWERED_N: f64 = 84.;

    pub fn new(number: usize) -> Aps3200ApuGenerator {
        Aps3200ApuGenerator {
            number,
            writer: ElectricalStateWriter::new(&format!("APU_GEN_{}", number)),
            output: Potential::None,
            random_voltage: TimedRandom::new(
                Duration::from_secs(1),
                vec![114., 115., 115., 115., 115.],
            ),
            current: ElectricCurrent::new::<ampere>(0.),
            potential: ElectricPotential::new::<volt>(0.),
            frequency: Frequency::new::<hertz>(0.),
        }
    }

    fn calculate_potential(&self, n: Ratio) -> ElectricPotential {
        let n = n.get::<percent>();

        if n < Aps3200ApuGenerator::APU_GEN_POWERED_N {
            panic!("Should not be invoked for APU N below {}", n);
        } else if n < 85. {
            ElectricPotential::new::<volt>(105.)
        } else {
            ElectricPotential::new::<volt>(self.random_voltage.current_value())
        }
    }

    fn calculate_frequency(&self, n: Ratio) -> Frequency {
        let n = n.get::<percent>();

        // Refer to APS3200.md for details on the values below and source data.
        if n < Aps3200ApuGenerator::APU_GEN_POWERED_N {
            panic!("Should not be invoked for APU N below {}", n);
        } else if n < 100. {
            const APU_FREQ_CONST: f64 = 1076894372064.8204;
            const APU_FREQ_X: f64 = -118009165327.71873;
            const APU_FREQ_X2: f64 = 5296044666.7118;
            const APU_FREQ_X3: f64 = -108419965.09400678;
            const APU_FREQ_X4: f64 = -36793.31899267512;
            const APU_FREQ_X5: f64 = 62934.36386220135;
            const APU_FREQ_X6: f64 = -1870.5197158547767;
            const APU_FREQ_X7: f64 = 31.376473743149806;
            const APU_FREQ_X8: f64 = -0.3510150716459761;
            const APU_FREQ_X9: f64 = 0.002726493614147866;
            const APU_FREQ_X10: f64 = -0.00001463272647792659;
            const APU_FREQ_X11: f64 = 0.00000005203375009496;
            const APU_FREQ_X12: f64 = -0.00000000011071318044;
            const APU_FREQ_X13: f64 = 0.00000000000010697005;

            Frequency::new::<hertz>(
                APU_FREQ_CONST
                    + (APU_FREQ_X * n)
                    + (APU_FREQ_X2 * n.powi(2))
                    + (APU_FREQ_X3 * n.powi(3))
                    + (APU_FREQ_X4 * n.powi(4))
                    + (APU_FREQ_X5 * n.powi(5))
                    + (APU_FREQ_X6 * n.powi(6))
                    + (APU_FREQ_X7 * n.powi(7))
                    + (APU_FREQ_X8 * n.powi(8))
                    + (APU_FREQ_X9 * n.powi(9))
                    + (APU_FREQ_X10 * n.powi(10))
                    + (APU_FREQ_X11 * n.powi(11))
                    + (APU_FREQ_X12 * n.powi(12))
                    + (APU_FREQ_X13 * n.powi(13)),
            )
        } else {
            Frequency::new::<hertz>(400.)
        }
    }
}
impl ApuGenerator for Aps3200ApuGenerator {
    fn update(&mut self, context: &UpdateContext, n: Ratio, is_emergency_shutdown: bool) {
        self.random_voltage.update(context);
        self.output = if is_emergency_shutdown
            || n.get::<percent>() < Aps3200ApuGenerator::APU_GEN_POWERED_N
        {
            Potential::None
        } else {
            Potential::ApuGenerator(self.number)
        };

        self.current = if self.is_powered() {
            // TODO: Once we actually know what to do with the amperes, we'll have to adapt this.
            ElectricCurrent::new::<ampere>(782.60)
        } else {
            ElectricCurrent::new::<ampere>(0.)
        };

        self.potential = if self.is_powered() {
            self.calculate_potential(n)
        } else {
            ElectricPotential::new::<volt>(0.)
        };

        self.frequency = if self.is_powered() {
            self.calculate_frequency(n)
        } else {
            Frequency::new::<hertz>(0.)
        };
    }
}
impl ProvidePotential for Aps3200ApuGenerator {
    fn potential(&self) -> ElectricPotential {
        self.potential
    }

    fn potential_normal(&self) -> bool {
        let volts = self.potential.get::<volt>();
        (110.0..=120.0).contains(&volts)
    }
}
impl ProvideFrequency for Aps3200ApuGenerator {
    fn frequency(&self) -> Frequency {
        self.frequency
    }

    fn frequency_normal(&self) -> bool {
        let hz = self.frequency.get::<hertz>();
        (390.0..=410.0).contains(&hz)
    }
}
impl ProvideLoad for Aps3200ApuGenerator {
    fn load(&self) -> Ratio {
        // TODO: Replace with actual values once calculated.
        Ratio::new::<percent>(0.)
    }

    fn load_normal(&self) -> bool {
        true
    }
}
impl PotentialSource for Aps3200ApuGenerator {
    fn output_potential(&self) -> Potential {
        self.output
    }
}
impl SimulationElement for Aps3200ApuGenerator {
    fn write(&self, writer: &mut SimulatorWriter) {
        self.writer.write_alternating_with_load(self, writer);
    }
}

#[cfg(test)]
mod apu_generator_tests {
    use ntest::assert_about_eq;
    use uom::si::frequency::hertz;

    use crate::{
        apu::tests::{tester, tester_with},
        simulation::{context, test::TestReaderWriter},
    };

    use super::*;

    #[test]
    fn starts_without_output() {
        assert!(apu_generator().is_unpowered());
    }

    #[test]
    fn when_apu_running_provides_output() {
        let mut generator = apu_generator();
        update_below_threshold(&mut generator);
        update_above_threshold(&mut generator);

        assert!(generator.is_powered());
    }

    #[test]
    fn when_apu_shutdown_provides_no_output() {
        let mut generator = apu_generator();
        update_above_threshold(&mut generator);
        update_below_threshold(&mut generator);

        assert!(generator.is_unpowered());
    }

    #[test]
    fn from_n_84_provides_voltage() {
        let mut tester = tester_with().starting_apu();

        loop {
            tester = tester.run(Duration::from_millis(50));

            let n = tester.n().get::<percent>();
            if n > 84. {
                assert!(tester.potential().get::<volt>() > 0.);
            }

            if (n - 100.).abs() < f64::EPSILON {
                break;
            }
        }
    }

    #[test]
    fn from_n_84_has_frequency() {
        let mut tester = tester_with().starting_apu();

        loop {
            tester = tester.run(Duration::from_millis(50));

            let n = tester.n().get::<percent>();
            if n > 84. {
                assert!(tester.frequency().get::<hertz>() > 0.);
            }

            if (n - 100.).abs() < f64::EPSILON {
                break;
            }
        }
    }

    #[test]
    fn in_normal_conditions_when_n_100_voltage_114_or_115() {
        let mut tester = tester_with().running_apu();

        for _ in 0..100 {
            tester = tester.run(Duration::from_millis(50));

            let voltage = tester.potential().get::<volt>();
            assert!((114.0..=115.0).contains(&voltage))
        }
    }

    #[test]
    fn in_normal_conditions_when_n_100_frequency_400() {
        let mut tester = tester_with().running_apu();

        for _ in 0..100 {
            tester = tester.run(Duration::from_millis(50));

            let frequency = tester.frequency().get::<hertz>();
            assert_about_eq!(frequency, 400.);
        }
    }

    #[test]
    fn when_shutdown_frequency_not_normal() {
        let tester = tester().run(Duration::from_secs(1_000));

        assert!(!tester.generator_frequency_within_normal_range());
    }

    #[test]
    fn when_running_frequency_normal() {
        let tester = tester_with().running_apu().run(Duration::from_secs(1_000));

        assert!(tester.generator_frequency_within_normal_range());
    }

    #[test]
    fn when_shutdown_potential_not_normal() {
        let tester = tester().run(Duration::from_secs(1_000));

        assert!(!tester.generator_potential_within_normal_range());
    }

    #[test]
    fn when_running_potential_normal() {
        let tester = tester_with().running_apu().run(Duration::from_secs(1_000));

        assert!(tester.generator_potential_within_normal_range());
    }

    #[test]
    fn when_apu_emergency_shutdown_provides_no_output() {
        let tester = tester_with()
            .running_apu()
            .and()
            .released_apu_fire_pb()
            .run(Duration::from_secs(1));

        assert!(tester.generator_output_potential().is_unpowered());
    }

    #[test]
    fn writes_its_state() {
        let apu_gen = apu_generator();
        let mut test_writer = TestReaderWriter::new();
        let mut writer = SimulatorWriter::new(&mut test_writer);

        apu_gen.write(&mut writer);

        assert!(test_writer.len_is(6));
        assert!(test_writer.contains_f64("ELEC_APU_GEN_1_POTENTIAL", 0.));
        assert!(test_writer.contains_bool("ELEC_APU_GEN_1_POTENTIAL_NORMAL", false));
        assert!(test_writer.contains_f64("ELEC_APU_GEN_1_FREQUENCY", 0.));
        assert!(test_writer.contains_bool("ELEC_APU_GEN_1_FREQUENCY_NORMAL", false));
        assert!(test_writer.contains_f64("ELEC_APU_GEN_1_LOAD", 0.));
        assert!(test_writer.contains_bool("ELEC_APU_GEN_1_LOAD_NORMAL", true));
    }

    fn apu_generator() -> Aps3200ApuGenerator {
        Aps3200ApuGenerator::new(1)
    }

    fn update_above_threshold(generator: &mut Aps3200ApuGenerator) {
        generator.update(&context(), Ratio::new::<percent>(100.), false);
    }

    fn update_below_threshold(generator: &mut Aps3200ApuGenerator) {
        generator.update(&context(), Ratio::new::<percent>(0.), false);
    }
}