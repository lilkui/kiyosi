macro_rules! analytic_engine {
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path) => {
        analytic_engine!($(#[$doc])* $engine, $instrument, $with, |_option, _context| false);
    };
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path,
     |$contract:ident, $market:ident| $short:expr) => {
        analytic_engine!(
            $(#[$doc])* $engine, $instrument, $with, |$contract, $market| $short,
            [
                ///
                /// Notes
                /// -----
                /// Before expiry, requested Greeks use scalar numerical differences with
                /// the request's shift controls. Omitted controls use engine defaults.
                ///
            ]
        );
    };
    ($(#[$doc:meta])* $engine:ident, $instrument:ty, $with:path,
     |$contract:ident, $market:ident| $short:expr,
     [$(#[$greeks_doc:meta])*]) => {
        $(#[$doc])*
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(PartialEq)]
        pub(crate) struct $engine;
        engine_methods!(@price $engine, $instrument,
            [
                $(#[$greeks_doc])*
            ],
            [],
            {
                #[classattr]
                const _STATE_FIELDS: &'static [&'static str] = &[];

                #[new]
                fn new() -> Self {
                    Self
                }
            }
        );
        impl $engine {
            pub(crate) fn evaluate<R: Send>(
                &self,
                py: Python<'_>,
                instrument: $instrument,
                context: &PricingContext,
                request: impl crate::engines::PricingRequest<R>,
            ) -> PyResult<R> {
                let $market = &context.0;
                let short = {
                    let $contract = &instrument;
                    $short
                };
                $with!(&instrument, $contract, domain(request.run(py,
                    short || kiyosi::OptionContract::expiry_time($contract) == $market.valuation_time(), || {
                    request.evaluate(&kiyosi::$engine, $contract, $market)
                })?))
            }
        }
    };
}

pub(crate) mod american;
pub(crate) mod asian;
pub(crate) mod barrier;
pub(crate) mod binary_barrier;
pub(crate) mod digital;
pub(crate) mod double_barrier;
pub(crate) mod double_touch;
pub(crate) mod european;
pub(crate) mod heston;
pub(crate) mod range_accrual;

fn short_barrier(
    observations: Option<&[kiyosi::Timestamp]>,
    closed_form: bool,
    context: &kiyosi::PricingContext,
) -> bool {
    // Bound time-stencil scans; integration and deterministic paths need cancellation.
    closed_form
        && observations.is_none_or(|times| times.len() <= 1024)
        && matches!(context.model_parameters(), kiyosi::ModelParameters::BlackScholesMerton(model)
            if model.volatility() > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_barrier_calls_bound_schedules_and_exclude_long_pricing_paths() -> kiyosi::Result<()> {
        let start = kiyosi::Date::new(2025, 1, 1)?.start_of_day();
        let context = kiyosi::PricingContext::new(
            kiyosi::BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?,
            100.0,
            start,
        )?;
        let times = (0..1025)
            .map(|offset| start.add_micros(offset))
            .collect::<kiyosi::Result<Vec<_>>>()?;
        assert!(short_barrier(None, true, &context));
        assert!(short_barrier(Some(&times[..1]), true, &context));
        assert!(short_barrier(Some(&times[..1024]), true, &context));
        assert!(!short_barrier(Some(&times), true, &context));
        assert!(!short_barrier(None, false, &context));
        assert!(!short_barrier(Some(&times[..1]), false, &context));
        let deterministic = context.replace(kiyosi::PricingContextChanges {
            model_parameters: Some(
                kiyosi::BlackScholesMertonParameters::new(0.05, 0.02, 0.0)?.into(),
            ),
            ..Default::default()
        })?;
        assert!(!short_barrier(None, true, &deterministic));
        assert!(!short_barrier(Some(&times[..1]), true, &deterministic));
        Ok(())
    }
}
