#pragma once

#include <algorithm>
#include <cmath>
#include <limits>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/pricing.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi::detail {

inline Result<double> binomial_time_step(const PricingContext& context, Date effective_date,
                                         Date expiry_date, BinomialSettings settings)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    return actual_365_fixed_year_fraction(context.valuation_time(), expiry_date) / static_cast<double>(settings.step_count);
}

template <typename Option>
Result<ImpliedVolatilitySettings> prepare_binomial_implied_volatility(
    BinomialSettings tree, const Option& option, const PricingContext& context, ImpliedVolatilitySettings settings)
{
    if (auto flat = require_flat_rates(context); !flat) return std::unexpected(flat.error());
    const auto step = binomial_time_step(context, option.effective_date(), option.expiry_date(), tree);
    if (!step) return std::unexpected(step.error());
    if (*step > 0.0) {
        const auto& parameters = context.model_parameters();
        const double drift = ((*parameters.risk_free_rate()) - (*parameters.dividend_yield())) * *step;
        // Round into the valid tree domain so the endpoint probability stays in [0, 1].
        const double minimum = std::nextafter(std::abs(drift) / std::sqrt(*step),
                                              std::numeric_limits<double>::infinity());
        if (std::isfinite(minimum)) settings.lower_bound = std::max(settings.lower_bound, minimum);
        if (settings.lower_bound >= settings.upper_bound)
            return std::unexpected(Error{ErrorCategory::unbracketed_volatility,
                                         "volatility bounds exclude the binomial tree domain"});
    }
    return settings;
}

} // namespace kiyosi::detail
