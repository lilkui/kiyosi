use super::{
    Error, FiniteDifferenceSettings, Greek, GreeksRequest, LocalDiffusion, OptionContract, Payoff,
    PricingContext, PricingResult, Result,
};
use crate::{LocalVolatilityParameters, ModelParameters};

pub(super) fn validate_request(context: &PricingContext, request: GreeksRequest) -> Result<()> {
    validate_model(context)?;
    if local_model(context).is_some()
        && Greek::ALL
            .into_iter()
            .any(|g| request.has(g) && !matches!(g, Greek::Delta | Greek::Gamma))
    {
        return Err(Error::UnsupportedOperation(
            "local volatility supports only delta and gamma".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_model(context: &PricingContext) -> Result<()> {
    if matches!(context.model_parameters(), ModelParameters::Heston(_)) {
        return Err(Error::UnsupportedOperation(
            "finite differences do not support Heston".into(),
        ));
    }
    Ok(())
}

pub(super) fn price(
    option: &impl OptionContract,
    payoff: Payoff,
    context: &PricingContext,
    model: &LocalVolatilityParameters,
    settings: FiniteDifferenceSettings,
    request: GreeksRequest,
) -> Result<PricingResult> {
    validate_request(context, request)?;
    Ok(super::native_model(
        option,
        payoff,
        context,
        settings,
        request,
        Default::default(),
        settings.asset_upper_boundary.is_none(),
        LocalDiffusion::new(context, model.surface()),
    )?
    .0)
}

pub(super) fn local_model(context: &PricingContext) -> Option<&LocalVolatilityParameters> {
    match context.model_parameters() {
        ModelParameters::LocalVolatility(m) => Some(m),
        ModelParameters::BlackScholesMerton(_) | ModelParameters::Heston(_) => None,
    }
}
