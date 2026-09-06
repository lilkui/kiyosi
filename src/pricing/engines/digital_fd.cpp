#include <kiyosi/pricing/engines/digital_fd.hpp>
#include <kiyosi/pricing/engines/digital.hpp>

namespace kiyosi {
result<PricingResult> FiniteDifferenceDigitalEngine::price(
    const EuropeanCashOrNothingOption& option, const PricingContext& context) const
{
    return AnalyticDigitalEngine{}.price(option, context);
}
result<PricingResult> FiniteDifferenceDigitalEngine::price(
    const EuropeanAssetOrNothingOption& option, const PricingContext& context) const
{
    return AnalyticDigitalEngine{}.price(option, context);
}
result<PricingResult> IntegralDigitalEngine::price(
    const EuropeanCashOrNothingOption& option, const PricingContext& context) const
{
    return AnalyticDigitalEngine{}.price(option, context);
}
result<PricingResult> IntegralDigitalEngine::price(
    const EuropeanAssetOrNothingOption& option, const PricingContext& context) const
{
    return AnalyticDigitalEngine{}.price(option, context);
}
} // namespace kiyosi
