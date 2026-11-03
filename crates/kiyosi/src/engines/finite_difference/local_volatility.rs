use super::{
    Error, FiniteDifferenceSettings, Greek, GreeksRequest, LocalDiffusion, OptionContract, Payoff,
    PricingContext, PricingResult, Result,
};
use crate::{LocalVolatilityParameters, ModelParameters};

pub(super) fn validate_request(
    context: &PricingContext,
    request: GreeksRequest,
) -> Result<GreeksRequest> {
    validate_model(context)?;
    Ok(if local_model(context).is_some() {
        GreeksRequest::new(
            [Greek::Delta, Greek::Gamma]
                .into_iter()
                .filter(|&g| request.has(g)),
        )
    } else {
        request
    })
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
    let request = validate_request(context, request)?;
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
