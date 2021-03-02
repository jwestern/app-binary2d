use serde::{Serialize, Deserialize};
use godunov_core::runge_kutta;
use crate::app::{
    AnyPrimitive,
};
use crate::mesh::{
    Mesh,
};
use crate::state::{
    ItemizedChange,
};
use crate::traits::{
    Arithmetic,
    Conserved,
    Hydrodynamics,
    Primitive,
    Zeros,
};


pub const ORBITAL_PERIOD: f64 = 2.0 * std::f64::consts::PI;


// ============================================================================
#[derive(thiserror::Error, Debug, Clone)]
pub enum HydroErrorType {

    #[error("negative surface density {0:.4e}")]
    NegativeDensity(f64),

    #[error("negative gas pressure {0:.4e}")]
    NegativePressure(f64),
}

impl HydroErrorType {
    pub fn at_position(self, position: (f64, f64)) -> HydroError {
        HydroError{source: self, binary: None, position}
    }
}




// ============================================================================
#[derive(thiserror::Error, Debug, Clone)]
#[error("at position ({:.4} {:.4}), when the binary was at ({:.4} {:.4}) ({:.4} {:.4})",
    position.0,
    position.1,
    binary.map_or(0.0, |s| s.0.position_x()),
    binary.map_or(0.0, |s| s.0.position_y()),
    binary.map_or(0.0, |s| s.1.position_x()),
    binary.map_or(0.0, |s| s.1.position_y()),
)]
pub struct HydroError {
    source: HydroErrorType,
    binary: Option<kepler_two_body::OrbitalState>,
    position: (f64, f64),
}

impl HydroError {
    pub fn with_orbital_state(self, binary: kepler_two_body::OrbitalState) -> Self {
        Self {
            source: self.source,
            binary: Some(binary),
            position: self.position,
        }
    }
}




// ============================================================================
#[derive(Copy, Clone)]
pub struct CellData<'a, P: Primitive> {
    pub pc: &'a P,
    pub gx: &'a P,
    pub gy: &'a P,
}




// ============================================================================
#[derive(Copy, Clone)]
pub enum Direction {
    X,
    Y,
}




// ============================================================================
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Physics {

    pub buffer_rate: f64,

    pub buffer_scale: f64,

    pub binary_eccentricity: f64,

    pub binary_mass_ratio: f64,

    #[serde(default)]
    pub lambda: f64,

    pub nu: f64,

    pub cfl: f64,

    pub plm: f64,

    /// Used to impose a "preemptive" density floor: fake mass is injected at
    /// the `fake_mass_rate` where the surface density is smaller than this
    /// dimensionless number, times the surface density at this position in
    /// the initial condition. When set to zero, the preemptive floor is never
    /// invoked. For brevity, this parameter may be omitted from the parameter
    /// file, and has a default value of zero.
    #[serde(default)]
    pub fake_mass_threshold: f64,

    /// The rate of injection of fake mass when the density is below the
    /// `fake_mass_threshold`. `Sigma_dot = fake_mass_rate * Omega *
    /// Sigma_background`, where `Sigma_background` is the surface density at
    /// this position in the initial condition. For brevity, this parameter
    /// may be omitted from the parameter file, and has a default value of
    /// zero.
    #[serde(default)]
    pub fake_mass_rate: f64,

    /// Whether to use a single point mass insead of a binary setup, intended
    /// mainly for testing.
    #[serde(default)]
    pub one_body: bool,

    /// An optional maximum Mach number, which the cooling prescription should
    /// avoid going above.
    pub mach_ceiling: Option<f64>,

    /// The Runge-Kutta order used for method-of-lines time integration.
    pub rk_order: runge_kutta::RungeKuttaOrder,

    /// The radius of a circular region around each component within which
    /// mass is subtracted. This is the length scale in the sink profile
    /// function, which is some type of super-Gaussian.
    pub sink_radius: f64,

    /// The amplitude of sink profile function.
    pub sink_rate: f64,
}




// ============================================================================
pub struct SourceTerms {
    pub fx1: f64,
    pub fy1: f64,
    pub fx2: f64,
    pub fy2: f64,
    pub sink_rate1: f64,
    pub sink_rate2: f64,
    pub buffer_rate: f64,
}




// ============================================================================
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Isothermal {
    pub mach_number: f64
}




// ============================================================================
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Euler {

    /// The adiabatic index, probably 5/3
    pub gamma_law_index: f64,

    /// Strength of the T^4 cooling
    pub cooling_coefficient: f64,
}




// ============================================================================
impl<'a, P: Primitive> CellData<'_, P> {

    pub fn new(pc: &'a P, gx: &'a P, gy: &'a P) -> CellData<'a, P> {
        CellData{
            pc: pc,
            gx: gx,
            gy: gy,
        }
    }

    pub fn stress_field(&self, nu: f64, lambda: f64, dx: f64, dy: f64, row: Direction, col: Direction) -> f64 {
        use Direction::{X, Y};

        let shear_stress = match (row, col) {
            (X, X) => 4.0 / 3.0 * self.gx.velocity_x() / dx - 2.0 / 3.0 * self.gy.velocity_y() / dy,
            (X, Y) => 1.0 / 1.0 * self.gx.velocity_y() / dx + 1.0 / 1.0 * self.gy.velocity_x() / dy,
            (Y, X) => 1.0 / 1.0 * self.gx.velocity_y() / dx + 1.0 / 1.0 * self.gy.velocity_x() / dy,
            (Y, Y) =>-2.0 / 3.0 * self.gx.velocity_x() / dx + 4.0 / 3.0 * self.gy.velocity_y() / dy,
        };

        let bulk_stress = match (row, col) {
            (X, X) => self.gx.velocity_x() / dx + self.gy.velocity_y() / dy,
            (X, Y) => 0.0,
            (Y, X) => 0.0,
            (Y, Y) => self.gx.velocity_x() / dx + self.gy.velocity_y() / dy,
        };

        self.pc.mass_density() * (nu * shear_stress + lambda * bulk_stress)
    }

    pub fn gradient_field(&self, axis: Direction) -> &P {
        use Direction::{X, Y};
        match axis {
            X => self.gx,
            Y => self.gy,
        }
    }
}




// ============================================================================
impl Physics {

    pub fn need_flux_communication(&self) -> bool {
        false
    }

    pub fn effective_resolution(&self, mesh: &Mesh) -> f64 {
        mesh.cell_spacing()
    }

    pub fn min_time_step(&self, mesh: &Mesh) -> f64 {
        self.cfl * self.effective_resolution(mesh) / self.maximum_orbital_velocity()
    }

    pub fn rk_substeps(&self) -> usize {
        match self.rk_order {
            runge_kutta::RungeKuttaOrder::RK1 => 1,
            runge_kutta::RungeKuttaOrder::RK2 => 2,
            runge_kutta::RungeKuttaOrder::RK3 => 3,
        }
    }
    pub fn sink_kernel(&self, dx: f64, dy: f64) -> f64 {
        let r2 = dx * dx + dy * dy;
        let s2 = self.sink_radius * self.sink_radius;

        if r2 < s2 * 9.0 {
            self.sink_rate * f64::exp(-(r2 / s2).powi(3))
        } else {
            0.0
        }
    }

    pub fn softening_length(&self) -> f64 {
        self.sink_radius
    }

    pub fn maximum_orbital_velocity(&self) -> f64 {
        1.0 / self.softening_length().sqrt()
    }

    pub fn orbital_elements(&self) -> kepler_two_body::OrbitalElements {
        if self.one_body {
            kepler_two_body::OrbitalElements(1e-12, 1.0, 1.0, 0.0)            
        } else {
            kepler_two_body::OrbitalElements(1.0, 1.0, self.binary_mass_ratio, self.binary_eccentricity)
        }
    }

    pub fn orbital_state_from_time(&self, time: f64) -> kepler_two_body::OrbitalState {
        self.orbital_elements().orbital_state_from_time(time)
    }

    pub fn source_terms(&self, mesh: &Mesh, two_body_state: &kepler_two_body::OrbitalState, x: f64, y: f64, surface_density: f64) -> SourceTerms {
        let p1 = two_body_state.0;
        let p2 = two_body_state.1;

        let [ax1, ay1] = p1.gravitational_acceleration(x, y, self.softening_length());
        let [ax2, ay2] = p2.gravitational_acceleration(x, y, self.softening_length());

        let fx1 = surface_density * ax1;
        let fy1 = surface_density * ay1;
        let fx2 = surface_density * ax2;
        let fy2 = surface_density * ay2;

        let x1 = p1.position_x();
        let y1 = p1.position_y();
        let x2 = p2.position_x();
        let y2 = p2.position_y();

        let sink_rate1 = self.sink_kernel(x - x1, y - y1);
        let sink_rate2 = self.sink_kernel(x - x2, y - y2);

        let r = (x * x + y * y).sqrt();
        let y = (r - mesh.domain_radius) / self.buffer_scale;
        let omega_outer = (two_body_state.total_mass() / mesh.domain_radius.powi(3)).sqrt();
        let buffer_rate = 0.5 * self.buffer_rate * (1.0 + f64::tanh(y)) * omega_outer;

        SourceTerms {
            fx1: fx1,
            fy1: fy1,
            fx2: fx2,
            fy2: fy2,
            sink_rate1: sink_rate1,
            sink_rate2: sink_rate2,
            buffer_rate: buffer_rate,
        }
    }

    pub fn fake_mass_threshold(&self) -> f64 {
        self.fake_mass_threshold
    }

    pub fn fake_mass_rate(&self) -> f64 {
        self.fake_mass_rate
    }
}




// ============================================================================
impl Hydrodynamics for Isothermal {

    type Conserved = hydro_iso2d::Conserved;
    type Primitive = hydro_iso2d::Primitive;

    fn gamma_law_index(&self) -> f64 {
        1.0
    }

    fn global_mach_number(&self) -> Option<f64> {
        Some(self.mach_number)
    }

    fn plm_gradient(&self, theta: f64, a: &Self::Primitive, b: &Self::Primitive, c: &Self::Primitive) -> Self::Primitive {
        godunov_core::piecewise_linear::plm_gradient3(theta, a, b, c)
    }

    fn try_to_primitive(&self, u: Self::Conserved) -> Result<Self::Primitive, HydroErrorType> {
        if u.density() < 0.0 {
            return Err(HydroErrorType::NegativeDensity(u.density()))
        }
        Ok(u.to_primitive())
    }

    fn to_primitive(&self, u: Self::Conserved) -> Self::Primitive {
        u.to_primitive()
    }

    fn to_conserved(&self, p: Self::Primitive) -> Self::Conserved {
        p.to_conserved()
    }

    fn from_any(&self, p: AnyPrimitive) -> Self::Primitive {
        hydro_iso2d::Primitive(
            p.surface_density,
            p.velocity_x,
            p.velocity_y,
        )
    }

    fn max_signal_speed(&self, u: Self::Conserved) -> f64 {
        let p = u.to_primitive();
        let vx = p.velocity_x().abs();
        let vy = p.velocity_y().abs();
        f64::max(vx, vy)
    }

    fn source_terms(
        &self,
        physics: &Physics,
        mesh: &Mesh,
        conserved: Self::Conserved,
        background_conserved: Self::Conserved,
        x: f64,
        y: f64,
        _t: f64,
        dt: f64,
        two_body_state: &kepler_two_body::OrbitalState) -> ItemizedChange<Self::Conserved>
    {
        let (u, u0) = (conserved, background_conserved);

        let fake_du = if u.density() < u0.density() * physics.fake_mass_threshold() {
            Self::Conserved::zeros()
        } else {
            u0 * physics.fake_mass_rate() * dt
        };
        let st = physics.source_terms(mesh, two_body_state, x, y, u.density());

        ItemizedChange {
            grav1:   hydro_iso2d::Conserved(0.0, st.fx1, st.fy1) * dt,
            grav2:   hydro_iso2d::Conserved(0.0, st.fx2, st.fy2) * dt,
            sink1:   u * (-st.sink_rate1 * dt),
            sink2:   u * (-st.sink_rate2 * dt),
            buffer: (u - u0) * (-dt * st.buffer_rate),
            cooling: Self::Conserved::zeros(),
            fake_mass: fake_du,
        }
    }

    fn intercell_flux<'a>(
        &self,
        physics: &Physics,
        l: &CellData<'a, hydro_iso2d::Primitive>,
        r: &CellData<'a, hydro_iso2d::Primitive>,
        dx: f64,
        dy: f64,
        gravitational_potential: f64,
        axis: Direction) -> hydro_iso2d::Conserved
    {
        let cs2 = -gravitational_potential / self.mach_number.powi(2);
        let pl  = *l.pc + *l.gradient_field(axis) * 0.5;
        let pr  = *r.pc - *r.gradient_field(axis) * 0.5;
        let nu  = physics.nu;
        let lam = physics.lambda;
        let tau_x = 0.5 * (l.stress_field(nu, lam, dx, dy, axis, Direction::X) + r.stress_field(nu, lam, dx, dy, axis, Direction::X));
        let tau_y = 0.5 * (l.stress_field(nu, lam, dx, dy, axis, Direction::Y) + r.stress_field(nu, lam, dx, dy, axis, Direction::Y));
        let iso2d_axis = match axis {
            Direction::X => hydro_iso2d::Direction::X,
            Direction::Y => hydro_iso2d::Direction::Y,
        };
        hydro_iso2d::riemann_hlle(pl, pr, iso2d_axis, cs2) + hydro_iso2d::Conserved(0.0, -tau_x, -tau_y)
    }
}




// ============================================================================
impl Hydrodynamics for Euler {

    type Conserved = hydro_euler::euler_2d::Conserved;
    type Primitive = hydro_euler::euler_2d::Primitive;

    fn gamma_law_index(&self) -> f64 {
        self.gamma_law_index
    }

    fn global_mach_number(&self) -> Option<f64> {
        None
    }

    fn plm_gradient(&self, theta: f64, a: &Self::Primitive, b: &Self::Primitive, c: &Self::Primitive) -> Self::Primitive {
        godunov_core::piecewise_linear::plm_gradient4(theta, a, b, c)
    }

    fn try_to_primitive(&self, u: Self::Conserved) -> Result<Self::Primitive, HydroErrorType> {
        if u.mass_density() < 0.0 {
            return Err(HydroErrorType::NegativeDensity(u.mass_density()))
        }
        let p = u.to_primitive(self.gamma_law_index);

        //if p.gas_pressure() < 0.0 {
        //    return Err(HydroErrorType::NegativePressure(p.gas_pressure()))
        //}

        if !(p.gas_pressure() > 0.0) {
            let pfixed = hydro_euler::euler_2d::Primitive(p.mass_density(), p.velocity_x(), p.velocity_y(), 1e-16);
            Ok(pfixed)
        }
        Ok(p)
    }

    fn to_primitive(&self, conserved: Self::Conserved) -> Self::Primitive {
        conserved.to_primitive(self.gamma_law_index)
    }

    fn to_conserved(&self, p: Self::Primitive) -> Self::Conserved {
        p.to_conserved(self.gamma_law_index)
    }

    fn from_any(&self, p: AnyPrimitive) -> Self::Primitive {
        hydro_euler::euler_2d::Primitive(
            p.surface_density,
            p.velocity_x,
            p.velocity_y,
            p.surface_pressure,
        )
    }

    fn max_signal_speed(&self, u: Self::Conserved) -> f64 {
        u.to_primitive(self.gamma_law_index).max_signal_speed(self.gamma_law_index)
    }

    fn source_terms(
        &self,
        physics: &Physics,
        mesh: &Mesh,
        u0: Self::Conserved,
        background_conserved: Self::Conserved,
        x: f64,
        y: f64,
        t: f64,
        dt: f64,
        two_body_state: &kepler_two_body::OrbitalState) -> ItemizedChange<Self::Conserved>
    {
        let st = physics.source_terms(mesh, two_body_state, x, y, u0.mass_density());
        let p0 = u0.to_primitive(self.gamma_law_index);
        let vx = p0.velocity_1();
        let vy = p0.velocity_2();
        let e0 = p0.specific_internal_energy(self.gamma_law_index);

        // The prescription below for the removal of thermal energy is
        // equivalent to Ryan & MacFadyen (2017). It gives accurate T^4
        // cooling, even when the cooling time is longer than the time step
        // dt, by integrating in time along the cooling curve. The cooling
        // prescription has two modes: one that assumes the photosphere
        // temperature is lower than the midplane temperature (given by the
        // primitive hydro variables) by a factor of the optical depth to the
        // 1/4 power, as described in Frank, King, and Raine Chapter 5, and
        // another that assumes the disk is vertically isothermal. With the
        // former prescription, the term `a` below is the coefficient in `L =
        // 2 sigma_SB T_phot^4 = Sigma de/dt = a e_mid^4` (note the factor of
        // two) since the disk has two surfaces.

        let density_index = 1; // Set to 2 to have the photosphere
                               // temperature smaller than the midplaned
                               // temperature by a factor of the optical
                               // depth.

        let f = f64::exp(-ORBITAL_PERIOD / (t + dt)); // slow-start term
        let a = self.cooling_coefficient / p0.mass_density().powi(density_index) * f;
        let ec = e0 * (1.0 + 3.0 * a * e0.powi(3) * dt).powf(-1.0 / 3.0);

        let ec = if let Some(mach_ceiling) = physics.mach_ceiling {
            let ek = p0.specific_kinetic_energy();
            let gm = self.gamma_law_index;
            ec.max(2.0 * ek / gm / (gm - 1.0) / mach_ceiling.powi(2))
        } else {
            ec
        };

        let pc = hydro_euler::euler_2d::Primitive(p0.0, p0.1, p0.2, p0.0 * ec * (self.gamma_law_index - 1.0));
        let uc = pc.to_conserved(self.gamma_law_index);

        ItemizedChange {
            grav1:     hydro_euler::euler_2d::Conserved(0.0, st.fx1, st.fy1, st.fx1 * vx + st.fy1 * vy) * dt,
            grav2:     hydro_euler::euler_2d::Conserved(0.0, st.fx2, st.fy2, st.fx2 * vx + st.fy2 * vy) * dt,
            sink1:     u0 * (-st.sink_rate1 * dt),
            sink2:     u0 * (-st.sink_rate2 * dt),
            buffer:   (u0 - background_conserved) * (-dt * st.buffer_rate),
            cooling:   uc - u0,
            fake_mass: Self::Conserved::zeros(),
        }
    }

    fn intercell_flux<'a>(
        &self,
        physics: &Physics,
        l: &CellData<'a, Self::Primitive>,
        r: &CellData<'a, Self::Primitive>,
        dx: f64,
        dy: f64,
        _gravitational_potential: f64,
        axis: Direction) -> Self::Conserved
    {
        let pl = *l.pc + *l.gradient_field(axis) * 0.5;
        let pr = *r.pc - *r.gradient_field(axis) * 0.5;

        let nu    = physics.nu;
        let lam   = physics.lambda;
        let tau_x = 0.5 * (l.stress_field(nu, lam, dx, dy, axis, Direction::X) + r.stress_field(nu, lam, dx, dy, axis, Direction::X));
        let tau_y = 0.5 * (l.stress_field(nu, lam, dx, dy, axis, Direction::Y) + r.stress_field(nu, lam, dx, dy, axis, Direction::Y));
        let vx = 0.5 * (l.pc.velocity_x() + r.pc.velocity_x());
        let vy = 0.5 * (l.pc.velocity_y() + r.pc.velocity_y());
        let viscous_flux = hydro_euler::euler_2d::Conserved(0.0, -tau_x, -tau_y, -(tau_x * vx + tau_y * vy));

        let euler_axis = match axis {
            Direction::X => hydro_euler::geometry::Direction::X,
            Direction::Y => hydro_euler::geometry::Direction::Y,
        };
        hydro_euler::euler_2d::riemann_hlle(pl, pr, euler_axis, self.gamma_law_index) + viscous_flux
    }
}




// ============================================================================
impl Arithmetic for hydro_iso2d::Conserved {}
impl Arithmetic for hydro_euler::euler_2d::Conserved {}




// ============================================================================
impl Zeros for hydro_iso2d::Conserved {
    fn zeros() -> Self {
        Self(0.0, 0.0, 0.0)
    }
}

impl Zeros for hydro_euler::euler_2d::Conserved {
    fn zeros() -> Self {
        Self(0.0, 0.0, 0.0, 0.0)
    }
}

impl Zeros for kepler_two_body::OrbitalElements {
    fn zeros() -> Self {
        Self(0.0, 0.0, 0.0, 0.0)
    }
}




// ============================================================================
impl Conserved for hydro_iso2d::Conserved {
    fn mass_and_momentum(&self) -> (f64, f64, f64) {
        (self.0, self.1, self.2)
    }
}

impl Conserved for hydro_euler::euler_2d::Conserved {
    fn mass_and_momentum(&self) -> (f64, f64, f64) {
        (self.0, self.1, self.2)
    }
}




// ============================================================================
impl Primitive for hydro_iso2d::Primitive {
    fn velocity_x(self)   -> f64 { self.velocity_x() }
    fn velocity_y(self)   -> f64 { self.velocity_y() }
    fn mass_density(self) -> f64 { self.density() }
}

impl Primitive for hydro_euler::euler_2d::Primitive {
    fn velocity_x(self)   -> f64 { self.velocity_1() }
    fn velocity_y(self)   -> f64 { self.velocity_2() }
    fn mass_density(self) -> f64 { self.mass_density() }
}
