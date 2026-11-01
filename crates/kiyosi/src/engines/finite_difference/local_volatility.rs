use super::*;
use crate::{LocalVolatilityParameters, ModelParameters};

pub(super) fn validate_request(context: &PricingContext, request: GreeksRequest) -> Result<()> {
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
        LocalDiffusion::new(context, model.surface()),
    )?
    .0)
}

pub(super) fn local_model(context: &PricingContext) -> Option<&LocalVolatilityParameters> {
    match context.model_parameters() {
        ModelParameters::LocalVolatility(m) => Some(m),
        _ => None,
    }
}
