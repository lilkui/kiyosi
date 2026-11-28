use crate::instruments::double_touch::DoubleTouchOption;
use crate::market::PricingContext;
use pyo3::prelude::*;

fn short(
    option: &kiyosi::DoubleTouchOption,
    context: &kiyosi::PricingContext,
) -> crate::engines::ShortPricing {
    use crate::engines::ShortPricing;

    let model = context.model_parameters();
    if option.observation_schedule().is_none()
        && super::short_barrier(
            None,
            option.settlement_timing() == kiyosi::SettlementTiming::AtExpiry,
            context,
        )
        && model.risk_free_curve().nodes().is_empty()
        && model.dividend_curve().nodes().is_empty()
    {
        ShortPricing::ScalarOnly
    } else {
        ShortPricing::Never
    }
}

analytic_engine!(
    /// Flat-rate continuous cash double-touch and double-no-touch pricing.
    ///
    /// Either inclusive barrier triggers a touch. Supports expiry payments and
    /// double-touch at-hit payments; past at-hit payments have zero remaining
    /// value. Uses image/spectral series and adaptive first-exit integration for
    /// at-hit payments. Nonconstant rate curves are rejected. At positive
    /// volatility, unresolved scheduled monitoring is unsupported. Resolved cash
    /// flows and zero-volatility paths support scheduled monitoring after calendar
    /// and history validation. Greeks use numerical differences. At expiry they
    /// follow terminal payoffs, with time Greeks absent. Before expiry at positive
    /// volatility, barrier equality without a prior touch leaves spot Greeks
    /// absent; rate and volatility Greeks follow the resolved payoff. Stencils
    /// crossing a barrier omit affected measures.
    ///
    /// See Also
    /// --------
    /// DoubleTouchOption : Supported cash contract.
    /// FiniteDifferenceDoubleTouchEngine : Supports schedules and nonconstant rate curves.
    ///
    AnalyticDoubleTouchEngine,
    PyRef<'_, DoubleTouchOption>,
    with_contract,
    |option, context| short(&option.0, context)
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{NativeGreeksRequest, ScalarRequest};
    use kiyosi::{
        BlackScholesMertonParameters, Date, DoubleBarrierType, DoubleTouchOptionTerms,
        SettlementTiming,
    };

    #[test]
    fn only_bounded_double_touch_scalar_prices_dispatch_inline() -> kiyosi::Result<()> {
        let start = Date::new(2025, 1, 1)?.start_of_day();
        let end = start.add_days(365)?;
        let context = |volatility| {
            kiyosi::PricingContext::new(
                BlackScholesMertonParameters::new(0.05, 0.02, volatility)?,
                100.0,
                start,
            )
        };
        let market = context(0.2)?;
        let terms =
            DoubleTouchOptionTerms::new(start, end, 80.0, 120.0, DoubleBarrierType::KnockIn);
        let option = kiyosi::DoubleTouchOption::cash(terms.clone(), 10.0)?;
        let policy = short(&option, &market);
        let greeks = NativeGreeksRequest {
            request: kiyosi::GreeksRequest::all(),
            settings: Default::default(),
        };
        assert!(policy.allows_inline(ScalarRequest::Price, &market));
        assert!(!policy.allows_inline(greeks, &market));
        let deterministic = context(0.0)?;
        assert!(
            !short(&option, &deterministic).allows_inline(ScalarRequest::Price, &deterministic)
        );
        let at_hit = kiyosi::DoubleTouchOption::cash(
            DoubleTouchOptionTerms {
                settlement_timing: SettlementTiming::AtHit,
                ..terms.clone()
            },
            10.0,
        )?;
        assert!(!short(&at_hit, &market).allows_inline(ScalarRequest::Price, &market));
        let scheduled = kiyosi::DoubleTouchOption::cash(
            DoubleTouchOptionTerms {
                observation_times: Some(vec![end]),
                ..terms
            },
            10.0,
        )?;
        assert!(!short(&scheduled, &market).allows_inline(ScalarRequest::Price, &market));
        Ok(())
    }
}
