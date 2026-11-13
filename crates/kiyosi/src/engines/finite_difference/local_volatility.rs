use super::{Error, Greek, GreeksRequest, OptionContract, PricingContext, Result};
use crate::{GreekMethod, LocalVolatilityParameters, ModelParameters};

pub(super) fn greek_method(
    option: &impl OptionContract,
    context: &PricingContext,
) -> Result<GreekMethod> {
    validate_model(context)?;
    Ok(GreekMethod::for_valuation(
        option,
        context,
        if local_model(context).is_some() {
            GreekMethod::NativeWithSpotControl
        } else {
            GreekMethod::Mixed
        },
    ))
}

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

pub(super) fn local_model(context: &PricingContext) -> Option<&LocalVolatilityParameters> {
    match context.model_parameters() {
        ModelParameters::LocalVolatility(m) => Some(m),
        ModelParameters::BlackScholesMerton(_) | ModelParameters::Heston(_) => None,
    }
}
