#include <kiyosi/pricing/engines/binary_barrier/finite_difference.hpp>

#include "../../detail/numerical_greeks.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <variant>
#include <vector>

#include <kiyosi/pricing/engines/digital/analytic.hpp>

#include "../../detail/black_scholes.hpp"
#include "../../detail/fd_event.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {

template <typename Option>
Result<PricingResult> price_binary_barrier(const Option& option, const PricingContext& context,
                                           FiniteDifferenceSettings settings, GreeksRequest output,
                                           double spot_shift = NumericalShiftSettings{}.spot_shift)
{
    constexpr bool touch = std::same_as<Option, TouchOption>;
    const auto& terms = option.barrier_terms();
    if (const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    const bool scheduled = option.observation_mode() == ObservationMode::scheduled;
    if (scheduled) {
        if (const auto valid = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar()); !valid)
            return std::unexpected(valid.error());
    }
    const auto state = barrier_monitoring_state(terms, context.valuation_time(), context.spot_price());
    if (!state) return std::unexpected(state.error());
    const bool asset_payoff = option.payoff_type() == PayoffType::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    const double cash_payout = cash ? cash->payout() : 0.0;
    const double spot = context.spot_price();
    const bool knock_in = terms.is_knock_in();
    const auto timing = [&] {
        if constexpr (touch) return option.settlement_timing();
        else return SettlementTiming::at_expiry;
    }();
    const Date horizon = [&] {
        if constexpr (touch)
            if (scheduled && timing == SettlementTiming::at_hit) return option.observation_dates().back();
        return option.expiry_date();
    }();
    const double maturity = actual_365_fixed_year_fraction(context.valuation_time(), horizon);
    const auto terminal = [&](double underlying) {
        if constexpr (touch) return asset_payoff ? underlying : cash_payout;
        else return binary_payoff_value(option.option_type(), option.strike(), cash_payout, asset_payoff, underlying);
    };
    if (state->touched_before && timing == SettlementTiming::at_hit)
        return make_pricing_result(0.0);
    if (maturity == 0.0)
        return make_pricing_result(state->touched == knock_in ? terminal(spot) : 0.0);
    const auto vanilla = [&]() -> Result<double> {
        if constexpr (touch) {
            const auto& curve = asset_payoff ? context.model_parameters().dividend_curve() : context.model_parameters().risk_free_curve();
            const auto discount = curve.log_factor(context.valuation_time(), start_of_day(option.expiry_date()));
            if (!discount) return std::unexpected(discount.error());
            return checked_price(scaled_exponential(asset_payoff ? spot : cash_payout, *discount));
        } else {
            const AnalyticDigitalEngine engine;
            if (asset_payoff)
                return engine.price(*make_asset_or_nothing_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()), context);
            return engine.price(*make_cash_or_nothing_option(option.option_type(), option.strike(), cash_payout, option.effective_date(), option.expiry_date()), context);
        }
    };
    if (state->touched || state->finished) {
        if (state->touched != knock_in) return make_pricing_result(0.0);
        if (state->touched && timing == SettlementTiming::at_hit)
            return make_pricing_result(asset_payoff ? spot : cash_payout);
        const auto price = vanilla();
        if (!price) return std::unexpected(price.error());
        return make_pricing_result(*price);
    }
    if (const auto valid = validate_curve_horizon(context, start_of_day(horizon)); !valid)
        return std::unexpected(valid.error());

    const auto& market = context.model_parameters();
    const double sigma = market.volatility();
    const double strike = [&] {
        if constexpr (touch) return option.barrier_level();
        else return option.strike();
    }();
    auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike, option.barrier_level()});
    if (!space) return std::unexpected(space.error());
    if (!scheduled) {
        const double lower = terms.is_up() ? 0.0 : terms.barrier_level();
        const double upper = terms.is_up() ? terms.barrier_level() : space->upper;
        space->upper = upper;
        space->nodes.clear();
        space->nodes.reserve(space->size());
        for (std::size_t index = 0; index < space->size(); ++index) {
            const double value = std::lerp(lower, upper, static_cast<double>(index) / settings.asset_step_count);
            if (!space->nodes.empty() && value <= space->nodes.back())
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "binary-barrier grid nodes are not representably distinct"});
            space->nodes.push_back(value);
        }
    }
    const auto grid = curve_finite_difference_time_grid(context, horizon, settings.time_step_count,
                                                        scheduled ? option.observation_dates() : std::vector<Date>{});
    const auto parameters = diffusion_parameters(context, scheme_theta(settings.scheme), 0.0, space->nodes.empty() ? nullptr : &*space);
    if (const auto stable = check_curve_explicit_stability(settings.scheme, grid, parameters, settings.asset_step_count, context); !stable)
        return std::unexpected(stable.error());
    std::vector<double> observations;
    if (scheduled)
        for (const Date date : option.observation_dates())
            if (date > context.valuation_time()) observations.push_back(actual_365_fixed_year_fraction(context.valuation_time(), date));
    const auto active = [&](double time) { return !scheduled || std::ranges::binary_search(observations, time); };
    const auto log_discount = [&](bool asset, double start, double end) {
        return curve_log_factor(asset ? market.dividend_curve() : market.risk_free_curve(), context.valuation_time(), start, end);
    };
    const auto base_value = [&](double underlying, double time) {
        const double tau = maturity - time;
        if (tau == 0.0) return terminal(underlying);
        const double discount = log_discount(asset_payoff, time, maturity);
        if constexpr (touch) return scaled_exponential(asset_payoff ? underlying : cash_payout, discount);
        else {
            if (underlying == 0.0)
                return option.option_type() == OptionType::put && !asset_payoff ? scaled_exponential(cash_payout, discount) : 0.0;
            const double rate = -log_discount(false, time, maturity) / tau;
            const double dividend = -log_discount(true, time, maturity) / tau;
            const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
            const auto probabilities = black_scholes_probabilities(sign, underlying, strike, rate, dividend, sigma, tau);
            return exponential_normal_cdf(std::log(asset_payoff ? underlying : cash_payout) + discount,
                                          sign * (asset_payoff ? probabilities.d1 : probabilities.d2));
        }
    };
    const auto hit_value = [&](double underlying, double time, double hit_time) {
        if constexpr (!touch) return 0.0;
        else {
            if (!knock_in) return 0.0;
            const double settlement = timing == SettlementTiming::at_hit ? hit_time : maturity;
            return scaled_exponential(asset_payoff ? underlying : cash_payout, log_discount(asset_payoff, time, settlement));
        }
    };
    const auto surviving_terminal = [&](double underlying) {
        if constexpr (touch) return knock_in ? 0.0 : terminal(underlying);
        else return terminal(underlying);
    };
    std::vector<double> breaks{strike, terms.barrier_level()};
    std::ranges::sort(breaks);
    breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
    std::vector<double> old(space->size());
    for (std::size_t index = 0; index < old.size(); ++index)
        old[index] = project_finite_difference_event<1>(*space, breaks, index, [&](double underlying) {
            return std::array{active(maturity) && terms.is_breached_by(underlying)
                                  ? hit_value(underlying, maturity, maturity)
                                  : surviving_terminal(underlying)};
        })[0];
    std::vector<double> vanilla_layer; // NOLINT(misc-const-correctness): populated only for binary knock-in instantiations.
    if constexpr (!touch) {
        if (knock_in) {
            vanilla_layer.resize(old.size());
            for (std::size_t index = 0; index < old.size(); ++index)
                vanilla_layer[index] = project_finite_difference_event<1>(*space, breaks, index, [&](double underlying) { return std::array{terminal(underlying)}; })[0];
            const auto boundary = [&](double tau) { return Boundaries{base_value(space->asset_at(0), maturity - tau), base_value(space->upper, maturity - tau)}; };
            if (const auto marched = march_backward<true>(grid, parameters, vanilla_layer, boundary, [](std::vector<double>&, double, double) {}, nullptr, &context); !marched)
                return std::unexpected(marched.error());
        }
    }
    const auto boundaries = [&](double time) {
        Boundaries result{base_value(space->asset_at(0), time), base_value(space->upper, time)};
        if constexpr (touch)
            if (knock_in) result = {};
        const auto next = std::ranges::lower_bound(observations, time);
        if (!scheduled || next != observations.end()) {
            const double hit_time = scheduled ? *next : time;
            if (terms.is_breached_by(space->asset_at(0))) result.lower = hit_value(space->asset_at(0), time, hit_time);
            if (terms.is_breached_by(space->upper)) result.upper = hit_value(space->upper, time, hit_time);
        }
        return result;
    };
    std::vector<double> next(old.size());
    FiniteDifferenceStep stepper(old.size());
    RannacherDamping damping{2};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const auto substeps = damping.next(parameters.theta);
        const double dt = grid.time_steps[step] / substeps.count;
        for (int substep = 0; substep < substeps.count; ++substep) {
            const double time = grid.times[step] + dt * (substeps.count - substep - 1);
            auto interval = diffusion_interval(parameters, context, time, time + dt);
            interval.theta = substeps.theta;
            const auto edges = boundaries(time);
            if (!stepper.advance(old, next, dt, interval, edges.lower, edges.upper))
                return std::unexpected(Error{ErrorCategory::invalid_result, "binary-barrier finite-difference system is numerically unstable"});
            old.swap(next);
        }
        const double time = grid.times[step];
        if (scheduled && active(time)) {
            const std::size_t cut = space->nearest_node(terms.barrier_level());
            const double correction = project_finite_difference_event<1>(*space, breaks, cut, [&](double underlying) {
                return std::array{terms.is_breached_by(underlying) ? hit_value(underlying, time, time) - space->linear_interpolate(old, underlying) : 0.0};
            })[0];
            const double projected = old[cut] + correction;
            for (std::size_t index = 0; index < old.size(); ++index)
                if (terms.is_breached_by(space->asset_at(index))) old[index] = hit_value(space->asset_at(index), time, time);
            old[cut] = projected;
            damping.restart();
        }
    }
    if constexpr (!touch)
        if (knock_in)
            for (std::size_t index = 0; index < old.size(); ++index)
                old[index] = std::max(vanilla_layer[index] - old[index], 0.0);
    std::vector<double> curvature;
    const auto price_at = [&](double value) { return checked_price(std::max(space->interpolate(old, value, curvature), 0.0)); };
    const auto value = price_at(spot);
    if (!value) return std::unexpected(value.error());
    return finite_difference_result(*value, spot, space->upper, output, spot_shift, price_at, [&](double value) { return space->delta(old, value, curvature); }, [&](double value) { return space->gamma(old, value, curvature); });
}

} // namespace

Result<double> FiniteDifferenceBinaryBarrierEngine::price(const BinaryBarrierOption& option, const PricingContext& context) const
{
    return price_value(price_binary_barrier(option, context, settings_, {}));
}

Result<double> FiniteDifferenceBinaryBarrierEngine::price(const TouchOption& option, const PricingContext& context) const
{
    return price_value(price_binary_barrier(option, context, settings_, {}));
}

Result<PricingResult> FiniteDifferenceBinaryBarrierEngine::price_with_greeks(const BinaryBarrierOption& option, const PricingContext& context,
                                                                             GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest requested) {
                                         return price_binary_barrier(option, shifted, engine.settings(), requested, settings.spot_shift);
                                     });
}

Result<PricingResult> FiniteDifferenceBinaryBarrierEngine::price_with_greeks(const TouchOption& option, const PricingContext& context,
                                                                             GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest requested) {
                                         return price_binary_barrier(option, shifted, engine.settings(), requested, settings.spot_shift);
                                     });
}

} // namespace kiyosi
