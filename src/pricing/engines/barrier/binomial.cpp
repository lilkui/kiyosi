#include <kiyosi/pricing/engines/barrier/binomial.hpp>

#include <array>
#include <cmath>
#include <cstdint>
#include <limits>
#include <numeric>
#include <vector>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/numerical_greeks.hpp"

namespace kiyosi {
namespace {

Result<PricingResult> price_barrier(const BarrierOption& option, const PricingContext& context,
                                    BinomialSettings settings, GreeksRequest greeks)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const auto& terms = option.barrier_terms();
    if (!terms.is_continuous()) {
        const auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    if (const auto settled = detail::resolved_barrier_price(option, context)) {
        if (!*settled) return std::unexpected(settled->error());
        const auto state = *detail::barrier_monitoring_state(terms, context.valuation_time(), context.spot_price());
        const bool vanilla = (state.touched && terms.is_knock_in()) || (!state.touched && state.finished && !terms.is_knock_in()) ||
                             (option.rebate() == 0.0 && terms.is_knock_in() && detail::terminal_payoff_requires_barrier_touch(terms, option.option_type(), option.strike()));
        std::array<Greek, 2> supported{};
        std::size_t count = 0;
        if (greeks.has(Greek::delta)) supported[count++] = Greek::delta;
        if (greeks.has(Greek::gamma) && settings.step_count >= 2) supported[count++] = Greek::gamma;
        if (vanilla)
            return detail::price_at_volatility(*make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
                                               context, context.model_parameters().volatility(), GreeksRequest{std::span{supported}.first(count)});
        return make_pricing_result(**settled, {{Greek::delta, greeks.has(Greek::delta) ? std::optional{0.0} : std::nullopt},
                                               {Greek::gamma, greeks.has(Greek::gamma) && settings.step_count >= 2 ? std::optional{0.0} : std::nullopt}});
    }
    const double time = detail::actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double spot = context.spot_price();
    if (time == 0.0) return make_pricing_result(terms.is_knock_in() ? option.rebate() : std::max(sign * (spot - option.strike()), 0.0));
    const auto steps = settings.step_count;
    std::vector<bool> observed(static_cast<std::size_t>(steps) + 1, terms.is_continuous());
    if (!terms.is_continuous()) {
        const auto duration = (start_of_day(option.expiry_date()) - context.valuation_time()).count();
        const auto divisor = std::gcd(duration, static_cast<std::int64_t>(steps));
        const auto quantum = duration / divisor;
        for (const auto date : option.observation_dates()) {
            const auto offset = (start_of_day(date) - context.valuation_time()).count();
            if (offset <= 0) continue;
            if (offset % quantum != 0)
                return std::unexpected(Error{ErrorCategory::unsupported_operation, "scheduled barrier observation does not coincide with a CRR tree step"});
            const auto index = (offset / quantum) * (steps / divisor);
            observed[static_cast<std::size_t>(index)] = true;
        }
    }
    const auto& model = context.model_parameters();
    const double dt = time / steps;
    const double log_up = model.volatility() * std::sqrt(dt);
    const double up = std::exp(log_up), down = 1.0 / up;
    const double discount = std::exp(-(*model.risk_free_rate()) * dt);
    const double probability = std::expm1(((*model.risk_free_rate()) - (*model.dividend_yield())) * dt + log_up) / std::expm1(2.0 * log_up);
    if (!std::isfinite(up) || !std::isfinite(down) || up == down || !std::isfinite(discount) ||
        !std::isfinite(probability) || probability < 0.0 || probability > 1.0)
        return std::unexpected(Error{ErrorCategory::invalid_result, "binomial parameters produced an unstable tree"});
    const double log_barrier = detail::log_price_ratio(spot, option.barrier_level());
    const auto breached = [&](int level, int node) {
        const double offset = (2.0 * node - level) * log_up;
        const double distance = log_barrier + offset;
        // Resolve rounded node equality in asset units, matching the core's inclusive trigger.
        if (std::abs(distance) <= 4.0 * std::numeric_limits<double>::epsilon() * std::max(1.0, std::abs(log_barrier) + std::abs(offset)))
            return terms.is_breached_by(detail::scaled_exponential(spot, offset));
        return terms.is_up() ? distance >= 0.0 : distance <= 0.0;
    };
    const auto rebate = [&](int level) {
        return option.rebate_timing() == RebateTiming::at_hit ? option.rebate() : detail::scaled_exponential(option.rebate(), -(*model.risk_free_rate()) * (steps - level) * dt);
    };
    std::vector<double> values(static_cast<std::size_t>(steps) + 1);
    std::vector<double> vanilla(terms.is_knock_in() ? values.size() : 0);
    for (int node = 0; node <= steps; ++node) {
        const double asset = detail::scaled_exponential(spot, (2.0 * node - steps) * log_up);
        if (!std::isnormal(asset)) return std::unexpected(Error{ErrorCategory::invalid_result, "binomial tree asset price overflowed or underflowed"});
        const double payoff = std::max(sign * (asset - option.strike()), 0.0);
        const bool touched = observed.back() && breached(steps, node);
        values[node] = terms.is_knock_in() ? (touched ? payoff : option.rebate()) : (touched ? option.rebate() : payoff);
        if (!vanilla.empty()) vanilla[node] = payoff;
    }
    std::array<double, 2> first{};
    std::array<double, 3> second{};
    const auto capture = [&](int level) {
        if (level == 1 && greeks.has(Greek::delta)) std::copy_n(values.begin(), 2, first.begin());
        if (level == 2 && greeks.has(Greek::gamma)) std::copy_n(values.begin(), 3, second.begin());
    };
    capture(steps);
    for (int level = steps - 1; level >= 0; --level) {
        const double event_rebate = observed[level] && !terms.is_knock_in() ? rebate(level) : 0.0;
        for (int node = 0; node <= level; ++node) {
            values[node] = discount * ((1.0 - probability) * values[node] + probability * values[node + 1]);
            if (!vanilla.empty()) vanilla[node] = discount * ((1.0 - probability) * vanilla[node] + probability * vanilla[node + 1]);
            if (observed[level] && breached(level, node)) values[node] = terms.is_knock_in() ? vanilla[node] : event_rebate;
            if (!std::isfinite(values[node]) || (!vanilla.empty() && !std::isfinite(vanilla[node])))
                return std::unexpected(Error{ErrorCategory::invalid_result, "binomial pricing produced a non-finite result"});
        }
        capture(level);
    }
    std::optional<double> delta, gamma;
    if (greeks.has(Greek::delta)) {
        const double width = spot * (up - down);
        if (std::isfinite(width) && width != 0.0) delta = (first[1] - first[0]) / width;
    }
    if (greeks.has(Greek::gamma) && steps >= 2) {
        const double upper_width = spot * (up * up - 1.0), lower_width = spot * (1.0 - down * down);
        const double width = 0.5 * spot * (up * up - down * down);
        if (std::isfinite(upper_width) && std::isfinite(lower_width) && std::isfinite(width) && upper_width != 0.0 && lower_width != 0.0 && width != 0.0)
            gamma = ((second[2] - second[1]) / upper_width - (second[1] - second[0]) / lower_width) / width;
    }
    return make_pricing_result(values[0], {{Greek::delta, delta}, {Greek::gamma, gamma}});
}
} // namespace

Result<double> CoxRossRubinsteinBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_value(price_barrier(option, context, settings_, {}));
}

Result<PricingResult> CoxRossRubinsteinBarrierEngine::price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto&) {
        return price_barrier(option, context, settings_, detail::greeks_unavailable(option, context) ? GreeksRequest{} : greeks);
    });
}
} // namespace kiyosi
