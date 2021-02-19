use serde::{Serialize, Deserialize};
use crate::app::AnyPrimitive;
use crate::traits::{Hydrodynamics, InitialModel};




#[derive(Clone, Serialize, Deserialize)]
pub struct FiniteDiskModel {

    width: f64,

    radius: f64,

    mach_number: Option<f64>,
}




#[derive(Clone, Serialize, Deserialize)]
pub struct InfiniteDiskModel {

    accretion_rate: f64,

    mach_number: Option<f64>,
}




// ============================================================================
impl InitialModel for FiniteDiskModel {

    fn primitive_at<H: Hydrodynamics>(&self, _hydro: &H, _xy: (f64, f64)) -> AnyPrimitive {
        todo!()
    }

    fn validate<H: Hydrodynamics>(&self, _hydro: &H) -> anyhow::Result<()> {
        Ok(())
    }
}




// ============================================================================
impl InitialModel for InfiniteDiskModel {

    fn primitive_at<H: Hydrodynamics>(&self, hydro: &H, xy: (f64, f64)) -> AnyPrimitive {
        let (x, y) = xy;
        let r = (x * x + y * y).sqrt();
        let sd = 1.0;
        let vp = r.powf(-0.5);
        let vx = vp * (-y / r);
        let vy = vp * ( x / r);

        let mach_number = hydro.global_mach_number().or(self.mach_number).unwrap();
        let cs = vp / mach_number;
        let gm = hydro.gamma_law_index();
        let pg = cs * cs * sd / gm;

        AnyPrimitive {
            velocity_x: vx,
            velocity_y: vy,
            surface_density: sd,
            surface_pressure: pg,
        }
    }

    fn validate<H: Hydrodynamics>(&self, hydro: &H) -> anyhow::Result<()> {
        match (self.mach_number, hydro.global_mach_number()) {
            (Some(_), Some(_)) | (None, None) => anyhow::bail!{
                "A Mach number must be specified in exactly one of [hydro, model]"
            },
            _ => Ok(())
        }
    }
}
