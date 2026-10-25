#pragma once

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <optional>
#include <ratio>
#include <span>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/instrument_analytics.hpp>
#include <kiyosi/pricing/detail/pricing.hpp>

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

#include "finite_difference_domain.hpp"

namespace kiyosi::detail {

template <typename Price>
[[nodiscard]] Result<double> shifted_value(
    const Price& price, const PricingContext& context, double spot,
    double volatility, double rate, Timestamp valuation)
{
    auto shifted = shifted_context(context, spot, volatility, rate, valuation);
    if (!shifted) return std::unexpected(shifted.error());
    const auto value = price(*shifted);
    if (!value) return std::unexpected(value.error());
    return checked_price(*value);
}

/// Extracts a scalar from the validated native engine result.
inline Result<double> price_value(const Result<PricingResult>& result)
{
    if (!result) return std::unexpected(result.error());
    return result->price();
}

inline Result<void> validate_greeks_settings(NumericalShiftSettings settings)
{
    if (!std::isfinite(settings.spot_shift) || settings.spot_shift <= 0.0 ||
        !std::isfinite(settings.volatility_shift) || settings.volatility_shift <= 0.0 ||
        !std::isfinite(settings.rate_shift) || settings.rate_shift <= 0.0 ||
        settings.time_shift_days <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "numerical shifts are invalid"});
    return {};
}

template <typename Option>
bool all_greeks_unavailable(const Option&, const PricingContext&) { return false; }

template <typename Option>
bool greeks_unavailable(const Option& option, const PricingContext& context)
{
    if constexpr (requires { option.expiry_date(); })
        if (context.valuation_time() == start_of_day(option.expiry_date())) return true;
    return all_greeks_unavailable(option, context);
}

template <typename Option>
bool at_spot_discontinuity(const Option&, const PricingContext&, double = 0.0) { return false; }

template <typename Option>
Result<bool> time_stencil_available(const Option&, const PricingContext&, Timestamp, Timestamp) { return true; }

struct NumericalStencils {
    double spot_shift;
    double volatility_shift;
    double rate_shift;
    bool spot_discontinuity;
    bool spot_available;
    bool speed_available;
    bool volatility_available;
    bool rate_available;
};

template <typename Option>
NumericalStencils numerical_stencils(const Option& option, const PricingContext& context,
                                     NumericalShiftSettings settings)
{
    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double h = symmetric_shift(spot, settings.spot_shift);
    const double v = symmetric_shift(volatility, settings.volatility_shift);
    const double r = symmetric_shift(rate_shift_origin(context.model_parameters()), settings.rate_shift);
    const bool spot_available = h > 0.0 && spot > h && !at_spot_discontinuity(option, context, h);
    const double two_h = 2.0 * h;
    return {h, v, r, at_spot_discontinuity(option, context), spot_available,
            spot_available && std::isfinite(two_h) && spot > two_h &&
                symmetric_shift(spot, two_h) == two_h && !at_spot_discontinuity(option, context, two_h),
            v > 0.0 && volatility > v && std::isfinite(100.0 * v),
            r > 0.0 && std::isfinite(200.0 * r)};
}

template <typename Option>
GreeksRequest native_greeks_request(const Option& option, const PricingContext& context,
                                    GreeksRequest greeks, double spot_radius)
{
    if (!at_spot_discontinuity(option, context, spot_radius)) return greeks;
    std::array<Greek, 3> available{};
    std::size_t count = 0;
    for (const auto greek : {Greek::vega, Greek::rho, Greek::theta})
        if (greeks.has(greek) && (greek != Greek::theta || !at_spot_discontinuity(option, context)))
            available[count++] = greek;
    return GreeksRequest{std::span{available}.first(count)};
}

struct NumericalSpotPrices {
    std::optional<double> center;
    double up;
    double down;
};

template <typename Price, typename SpotBatch>
Result<NumericalSpotPrices> sample_spot_prices(
    const Price& price, const SpotBatch& batch, const PricingContext& context, double h, bool center,
    std::optional<double> known_center = std::nullopt)
{
    const auto sampled = [&]() -> Result<NumericalSpotPrices> {
        if constexpr (requires { batch(context, h, center); }) {
            auto values = batch(context, h, center && !known_center);
            if (values && known_center) values->center = known_center;
            return values;
        } else {
            std::optional<double> middle = known_center;
            if (center && !middle) {
                const auto value = price(context);
                if (!value) return std::unexpected(value.error());
                middle = *value;
            }
            const double volatility = context.model_parameters().volatility();
            const double rate = rate_shift_origin(context.model_parameters());
            const auto up = shifted_value(price, context, context.spot_price() + h, volatility, rate, context.valuation_time());
            if (!up) return std::unexpected(up.error());
            const auto down = shifted_value(price, context, context.spot_price() - h, volatility, rate, context.valuation_time());
            if (!down) return std::unexpected(down.error());
            return NumericalSpotPrices{middle, *up, *down};
        }
    }();
    if (!sampled) return std::unexpected(sampled.error());
    if ((center && !sampled->center) || !std::isfinite(sampled->up) ||
        !std::isfinite(sampled->down) || (sampled->center && !std::isfinite(*sampled->center)))
        return std::unexpected(Error{ErrorCategory::invalid_result, "numerical spot batch is incomplete or non-finite"});
    return sampled;
}

struct MixedGreekSamples {
    std::array<double, 2> prices;
    std::optional<std::array<NumericalSpotPrices, 2>> spots;
};

template <typename Sample, typename Price>
Result<MixedGreekSamples> sample_mixed_greeks(
    std::array<const PricingResult*, 2> native, bool scalar_mixed, bool need_prices,
    double base_price, const Sample& sample, const Price& price)
{
    MixedGreekSamples result{};
    if (scalar_mixed) {
        const auto first = sample(0, need_prices && !native[0]);
        if (!first) return std::unexpected(first.error());
        const auto last = sample(1, need_prices && !native[1]);
        if (!last) return std::unexpected(last.error());
        result.spots = std::array{*first, *last};
    }
    for (std::size_t index = 0; index < result.prices.size(); ++index) {
        const auto value = native[index]  ? Result<double>{native[index]->price()}
                           : result.spots ? Result<double>{(*result.spots)[index].center.value_or(base_price)}
                           : need_prices  ? price(index)
                                          : Result<double>{base_price};
        if (!value) return std::unexpected(value.error());
        result.prices[index] = *value;
    }
    return result;
}

// Enforces boundary availability, then fills only missing requested measures.
// Away from unavailable boundaries, a supplied native value is never overwritten.
template <typename Price, typename Option, typename Native = decltype(nullptr), typename SpotBatch = decltype(nullptr), typename VolatilityBatch = decltype(nullptr)>
Result<PricingResult> complete_greeks(
    const Price& price, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const PricingResult& native,
    const Native& evaluate_native = nullptr, const SpotBatch& evaluate_spot_batch = nullptr,
    const VolatilityBatch& evaluate_volatility_batch = nullptr)
{
    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double rate = rate_shift_origin(context.model_parameters());
    const Timestamp valuation_time = context.valuation_time();
    const double p0 = native.price();
    if (greeks_unavailable(option, context))
        return make_pricing_result(p0);
    const auto stencils = numerical_stencils(option, context, settings);
    const auto native_request = native_greeks_request(option, context, true, stencils.spot_shift);
    const auto native_has = [&](Greek measure) { return native_request.has(measure) && native.has(measure); };
    const auto native_value = [&](Greek measure) { return native_has(measure) ? *native.get(measure) : std::nullopt; };
    const auto need = [&](Greek measure) { return greeks.has(measure) && !native_has(measure); };
    const double h = stencils.spot_shift;
    const bool spot_discontinuity = stencils.spot_discontinuity;
    const bool spot_stencil_available = stencils.spot_available;
    const double two_h = 2.0 * h;
    const bool scalar_speed_available = need(Greek::speed) && stencils.speed_available;
    constexpr bool native_stencil = requires { evaluate_native(context, GreeksRequest{}); };
    const auto checked_native = [&](const auto& shifted, GreeksRequest derivatives) -> Result<PricingResult> {
        derivatives = native_greeks_request(option, shifted, derivatives,
                                            symmetric_shift(shifted.spot_price(), settings.spot_shift));
        auto result = evaluate_native(shifted, derivatives);
        if (!result) return result;
        *result = result->selected(derivatives);
        return result;
    };
    auto delta = native_value(Greek::delta);
    auto gamma = native_value(Greek::gamma);
    auto speed = native_value(Greek::speed);
    std::optional<double> spot_up;
    std::optional<double> spot_down;
    if (spot_stencil_available && (need(Greek::delta) || need(Greek::gamma) ||
                                   (need(Greek::speed) && (scalar_speed_available || native_stencil)))) {
        std::optional<std::array<PricingResult, 2>> native_spots;
        if constexpr (requires { evaluate_native(context, GreeksRequest{}); }) {
            if (need(Greek::speed)) {
                const auto evaluate = [&](double shifted_spot) -> Result<PricingResult> {
                    const auto shifted = shifted_context(context, shifted_spot, volatility, rate, valuation_time);
                    if (!shifted) return std::unexpected(shifted.error());
                    return checked_native(*shifted, GreeksRequest{Greek::gamma});
                };
                const auto high = evaluate(spot + h);
                if (!high) return std::unexpected(high.error());
                const auto low = evaluate(spot - h);
                if (!low) return std::unexpected(low.error());
                native_spots = std::array{*high, *low};
            }
        }
        const auto p_up = native_spots ? Result<double>{(*native_spots)[0].price()} : detail::shifted_value(price, context, spot + h, volatility, rate, valuation_time);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = native_spots ? Result<double>{(*native_spots)[1].price()} : detail::shifted_value(price, context, spot - h, volatility, rate, valuation_time);
        if (!p_down) return std::unexpected(p_down.error());
        spot_up = *p_up;
        spot_down = *p_down;
        if (need(Greek::delta)) delta = (*p_up - *p_down) / (2.0 * h);
        // Subtract nearby prices before weighting; divide separately to preserve the shift's range.
        if (need(Greek::gamma)) gamma = ((*p_up - p0) - (p0 - *p_down)) / h / h;

        const bool native_speed = native_spots && (*native_spots)[0].has(Greek::gamma) && (*native_spots)[1].has(Greek::gamma);
        if (need(Greek::speed) && native_speed)
            speed = (*(*native_spots)[0].require(Greek::gamma) - *(*native_spots)[1].require(Greek::gamma)) / (2.0 * h);

        if (need(Greek::speed) && !native_speed && scalar_speed_available) {
            const auto p_up2 = detail::shifted_value(
                price, context, spot + two_h, volatility, rate, valuation_time);
            if (!p_up2) return std::unexpected(p_up2.error());
            const auto p_down2 = detail::shifted_value(
                price, context, spot - two_h, volatility, rate, valuation_time);
            if (!p_down2) return std::unexpected(p_down2.error());
            speed = ((*p_up2 - *p_up) - (*p_up - *p_down) + (*p_down - *p_down2)) /
                    h / h / two_h;
        }
    }

    auto vega = native_value(Greek::vega);
    auto vanna = native_value(Greek::vanna);
    auto zomma = native_value(Greek::zomma);
    const double volatility_shift = stencils.volatility_shift;
    const double vol_scale = 100.0 * volatility_shift;
    const bool volatility_stencil_available = stencils.volatility_available;
    if (volatility_stencil_available &&
        (need(Greek::vega) || (spot_stencil_available && (need(Greek::vanna) || need(Greek::zomma))))) {
        const double volatility_high = volatility + volatility_shift;
        const double volatility_low = volatility - volatility_shift;
        std::optional<std::array<PricingResult, 2>> native_volatilities;
        if constexpr (requires { evaluate_native(context, GreeksRequest{}); }) {
            if (spot_stencil_available && (need(Greek::vanna) || need(Greek::zomma))) {
                const auto evaluate = [&](double shifted_volatility) -> Result<PricingResult> {
                    const auto shifted = shifted_context(context, spot, shifted_volatility, rate, valuation_time);
                    if (!shifted) return std::unexpected(shifted.error());
                    const GreeksRequest derivatives = need(Greek::vanna)
                                                          ? (need(Greek::zomma) ? GreeksRequest{Greek::delta, Greek::gamma} : GreeksRequest{Greek::delta})
                                                          : GreeksRequest{Greek::gamma};
                    return checked_native(*shifted, derivatives);
                };
                const auto high = evaluate(volatility_high);
                if (!high) return std::unexpected(high.error());
                const auto low = evaluate(volatility_low);
                if (!low) return std::unexpected(low.error());
                native_volatilities = std::array{*high, *low};
            }
        }
        const bool native_vanna = native_volatilities && (*native_volatilities)[0].has(Greek::delta) && (*native_volatilities)[1].has(Greek::delta);
        const bool native_zomma = native_volatilities && (*native_volatilities)[0].has(Greek::gamma) && (*native_volatilities)[1].has(Greek::gamma);
        const bool scalar_mixed = spot_stencil_available && ((need(Greek::vanna) && !native_vanna) || (need(Greek::zomma) && !native_zomma));
        const bool need_volatility_prices = need(Greek::vega) || need(Greek::zomma);
        const std::array shifted_volatilities{volatility_high, volatility_low};
        const auto sampled = [&]() -> Result<MixedGreekSamples> {
            if constexpr (requires { evaluate_volatility_batch(h, need_volatility_prices); }) {
                if (scalar_mixed) {
                    const auto spots = evaluate_volatility_batch(h, need_volatility_prices);
                    if (!spots) return std::unexpected(spots.error());
                    return MixedGreekSamples{{(*spots)[0].center.value_or(p0), (*spots)[1].center.value_or(p0)}, *spots};
                }
            }
            return sample_mixed_greeks(
                {native_volatilities ? native_volatilities->data() : nullptr, native_volatilities ? &(*native_volatilities)[1] : nullptr},
                scalar_mixed, need_volatility_prices, p0,
                [&](std::size_t index, bool center) -> Result<NumericalSpotPrices> {
                    const double shifted_volatility = shifted_volatilities[index];
                    const auto shifted = shifted_context(context, spot, shifted_volatility, rate, valuation_time);
                    if (!shifted) return std::unexpected(shifted.error());
                    return sample_spot_prices(price, evaluate_spot_batch, *shifted, h, center);
                },
                [&](std::size_t index) { return detail::shifted_value(price, context, spot, shifted_volatilities[index], rate, valuation_time); });
        }();
        if (!sampled) return std::unexpected(sampled.error());
        const auto [v_up, v_down] = sampled->prices;
        if (need(Greek::vega)) vega = (v_up - v_down) / (2.0 * vol_scale);

        if (need(Greek::vanna) && native_vanna)
            vanna = (*(*native_volatilities)[0].require(Greek::delta) - *(*native_volatilities)[1].require(Greek::delta)) / (2.0 * vol_scale);
        if (need(Greek::zomma) && native_zomma)
            zomma = (*(*native_volatilities)[0].require(Greek::gamma) - *(*native_volatilities)[1].require(Greek::gamma)) / (2.0 * vol_scale);

        if (sampled->spots) {
            const auto& high = (*sampled->spots)[0];
            const auto& low = (*sampled->spots)[1];
            if (need(Greek::vanna) && !native_vanna) vanna = ((high.up - high.down) - (low.up - low.down)) /
                                                             h / vol_scale / 4.0;

            if (need(Greek::zomma) && !native_zomma) {
                const double gamma_high = ((high.up - v_up) - (v_up - high.down)) / h / h;
                const double gamma_low = ((low.up - v_down) - (v_down - low.down)) / h / h;
                zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);
            }
        }
    }

    auto rho = native_value(Greek::rho);
    const double rate_shift = stencils.rate_shift;
    const double rate_scale = 200.0 * rate_shift;
    if (need(Greek::rho) && stencils.rate_available) {
        const auto r_up = detail::shifted_value(
            price, context, spot, volatility, rate + rate_shift, valuation_time);
        if (!r_up) return std::unexpected(r_up.error());
        const auto r_down = detail::shifted_value(
            price, context, spot, volatility, rate - rate_shift, valuation_time);
        if (!r_down) return std::unexpected(r_down.error());
        rho = (*r_up - *r_down) / rate_scale;
    }

    auto theta = native_value(Greek::theta);
    auto charm = native_value(Greek::charm);
    auto color = native_value(Greek::color);
    if (need(Greek::theta) || need(Greek::charm) || need(Greek::color)) {
        using Days = std::chrono::duration<double, std::ratio<86400>>;
        auto lower_bound = start_of_day(Date{std::chrono::year::min() / std::chrono::January / 1});
        auto upper_bound = start_of_day(Date{std::chrono::year::max() / std::chrono::December / 31}) +
                           std::chrono::days{1} - std::chrono::microseconds{1};
        // Range accrual also supports valuation before its effective date.
        if constexpr (requires { option.effective_date(); } && !std::same_as<Option, RangeAccrualOption>)
            lower_bound = start_of_day(option.effective_date());
        if constexpr (requires { option.expiry_date(); })
            upper_bound = start_of_day(option.expiry_date());
        for (const auto* curve : {&context.model_parameters().risk_free_curve(), &context.model_parameters().dividend_curve()}) {
            if (const auto reference = curve->reference_time()) lower_bound = std::max(lower_bound, *reference);
            if (const auto last = curve->max_time()) upper_bound = std::min(upper_bound, *last);
            for (const Timestamp knot : curve->nodes()) {
                if (knot <= valuation_time) lower_bound = std::max(lower_bound, knot);
                else upper_bound = std::min(upper_bound, knot);
            }
        }
        // Bound days before conversion, retaining subday intervals between curve knots.
        const auto available_width = std::max(Timestamp::duration::zero(), upper_bound - lower_bound);
        const auto requested_shift = std::min(available_width, std::chrono::duration_cast<Timestamp::duration>(
                                                                   std::min(std::chrono::days{settings.time_shift_days}, std::max(std::chrono::days{1}, std::chrono::floor<std::chrono::days>(available_width)))));
        const auto available_before = valuation_time - lower_bound;
        const auto available_after = upper_bound - valuation_time;
        const auto shift = available_before > Timestamp::duration::zero() && available_after > Timestamp::duration::zero()
                               ? std::min({requested_shift, available_before, available_after})
                               : requested_shift;
        const Timestamp before = valuation_time - std::min(shift, available_before);
        const Timestamp after = valuation_time + std::min(shift, available_after);
        const auto time_available = time_stencil_available(option, context, before, after);
        if (!time_available) return std::unexpected(time_available.error());
        if (!spot_discontinuity && *time_available && before != after) {
            std::optional<std::array<PricingResult, 2>> native_times;
            if constexpr (requires { evaluate_native(context, GreeksRequest{}); }) {
                const bool native_time_stencil_available = (before != valuation_time && after != valuation_time) ||
                                                           ((!need(Greek::charm) || native_has(Greek::delta)) && (!need(Greek::color) || native_has(Greek::gamma)));
                if (spot_stencil_available && native_time_stencil_available && (need(Greek::charm) || need(Greek::color))) {
                    const auto evaluate = [&](Timestamp time) -> Result<PricingResult> {
                        if (time == valuation_time) return native;
                        const auto shifted = shifted_context(context, spot, volatility, rate, time);
                        if (!shifted) return std::unexpected(shifted.error());
                        const GreeksRequest derivatives = need(Greek::charm)
                                                              ? (need(Greek::color) ? GreeksRequest{Greek::delta, Greek::gamma} : GreeksRequest{Greek::delta})
                                                              : GreeksRequest{Greek::gamma};
                        return checked_native(*shifted, derivatives);
                    };
                    const auto first = evaluate(before);
                    if (!first) return std::unexpected(first.error());
                    const auto last = evaluate(after);
                    if (!last) return std::unexpected(last.error());
                    native_times = std::array{*first, *last};
                }
            }
            const bool native_charm = native_times && (*native_times)[0].has(Greek::delta) && (*native_times)[1].has(Greek::delta);
            const bool native_color = native_times && (*native_times)[0].has(Greek::gamma) && (*native_times)[1].has(Greek::gamma);
            const bool scalar_mixed = spot_stencil_available && ((need(Greek::charm) && !native_charm) || (need(Greek::color) && !native_color));
            const bool need_time_prices = need(Greek::theta) || (spot_stencil_available && need(Greek::color) && !native_color);
            const std::array shifted_times{before, after};
            const auto sampled = sample_mixed_greeks(
                {native_times ? native_times->data() : nullptr, native_times ? &(*native_times)[1] : nullptr},
                scalar_mixed, need_time_prices, p0,
                [&](std::size_t index, bool center) -> Result<NumericalSpotPrices> {
                    const Timestamp time = shifted_times[index];
                    if (time == valuation_time && spot_up && spot_down)
                        return NumericalSpotPrices{p0, *spot_up, *spot_down};
                    const auto shifted = shifted_context(context, spot, volatility, rate, time);
                    if (!shifted) return std::unexpected(shifted.error());
                    return sample_spot_prices(price, evaluate_spot_batch, *shifted, h, center,
                                              time == valuation_time ? std::optional{p0} : std::nullopt);
                },
                [&](std::size_t index) {
                    const Timestamp time = shifted_times[index];
                    return time == valuation_time ? p0 : detail::shifted_value(price, context, spot, volatility, rate, time);
                });
            if (!sampled) return std::unexpected(sampled.error());
            const auto [t_before, t_after] = sampled->prices;
            const double day_scale = Days{after - before}.count();
            if (need(Greek::theta)) theta = (t_after - t_before) / day_scale;
            if (need(Greek::charm) && native_charm)
                charm = (*(*native_times)[1].require(Greek::delta) - *(*native_times)[0].require(Greek::delta)) / day_scale;
            if (need(Greek::color) && native_color)
                color = (*(*native_times)[1].require(Greek::gamma) - *(*native_times)[0].require(Greek::gamma)) / day_scale;

            if (sampled->spots) {
                const auto& first = (*sampled->spots)[0];
                const auto& last = (*sampled->spots)[1];
                if (need(Greek::charm) && !native_charm) charm = ((last.up - last.down) - (first.up - first.down)) /
                                                                 h / day_scale / 2.0;
                if (need(Greek::color) && !native_color) color = ((((last.up - t_after) - (t_after - last.down)) -
                                                                   ((first.up - t_before) - (t_before - first.down))) /
                                                                  h / h / day_scale);
            }
        }
    }
    auto output = make_pricing_result(p0, {{Greek::delta, delta}, {Greek::gamma, gamma}, {Greek::speed, speed}, {Greek::theta, theta}, {Greek::charm, charm}, {Greek::color, color}, {Greek::vega, vega}, {Greek::vanna, vanna}, {Greek::zomma, zomma}, {Greek::rho, rho}});
    if (!output) return std::unexpected(output.error());
    *output = output->selected(greeks);
    return output;
}

// Validate and stabilize a request whose evaluator owns Greek calculation and completion.
template <typename Engine, typename Option, typename Evaluate>
Result<PricingResult> evaluate_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const Evaluate& evaluate)
{
    const auto request_valid = greeks.validate();
    if (!request_valid) return std::unexpected(request_valid.error());
    const auto valid = validate_greeks_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    auto result = with_monte_carlo_seed<Engine, Option>(engine, [&](const Engine& seeded) {
        if constexpr (is_builtin_finite_difference_engine<Engine, Option>) {
            auto grid = seeded.settings();
            if (!grid.asset_upper_boundary && validate_curve_horizon(context, start_of_day(option.expiry_date()))) {
                // Spot bumps share one mesh; settlements may never construct a grid.
                const double upper = default_finite_difference_upper_boundary(option, context);
                if (std::isfinite(upper)) {
                    grid.asset_upper_boundary = upper;
                    const auto fixed = make_engine(grid, std::type_identity<Engine>{});
                    if (!fixed) return decltype(evaluate(seeded)){std::unexpected(fixed.error())};
                    return evaluate(*fixed);
                }
            }
        }
        return evaluate(seeded);
    });
    if (!result) return result;
    *result = result->selected(greeks);
    if (greeks_unavailable(option, context)) return make_pricing_result(result->price());
    return result;
}

// Native callbacks have one protocol: engine, shifted context, requested derivatives.
template <typename Engine, typename Option, typename Native>
Result<PricingResult> price_with_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const Native& evaluate_native)
{
    return evaluate_greeks(engine, option, context, greeks, settings, [&](const Engine& fixed) -> Result<PricingResult> {
        auto native_request = greeks;
        if constexpr (requires { option.effective_date(); }) {
            if (context.valuation_time() == start_of_day(option.effective_date()) &&
                (greeks.has(Greek::charm) || greeks.has(Greek::color))) {
                // One-sided time differences need the central spot derivatives too.
                std::array<Greek, greek_count> measures{};
                std::size_t count = 0;
                for (std::size_t index = 0; index < greek_count; ++index) {
                    const auto greek = static_cast<Greek>(index);
                    if (greeks.has(greek) || (greek == Greek::delta && greeks.has(Greek::charm)) ||
                        (greek == Greek::gamma && greeks.has(Greek::color)))
                        measures[count++] = greek;
                }
                native_request = GreeksRequest{std::span{measures}.first(count)};
            }
        }
        native_request = native_greeks_request(option, context, native_request,
                                               symmetric_shift(context.spot_price(), settings.spot_shift));
        auto native = evaluate_native(fixed, context, native_request);
        if (!native) return native;
        *native = native->selected(native_request);
        const auto price = [&](const PricingContext& shifted) {
            if constexpr (is_builtin_finite_difference_engine<Engine, Option>) {
                return price_value(evaluate_native(fixed, shifted, GreeksRequest{}));
            } else {
                return fixed.price(option, shifted);
            }
        };
        return complete_greeks(price, option, context, greeks, settings, *native,
                               [&](const PricingContext& shifted, GreeksRequest request) {
                                   return evaluate_native(fixed, shifted, request);
                               });
    });
}

template <typename Engine, typename Option>
Result<PricingResult> price_with_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings)
{
    return evaluate_greeks(engine, option, context, greeks, settings,
                           [&](const Engine& fixed) -> Result<PricingResult> {
                               const auto value = numerical_value(fixed, option, context);
                               if (!value) return std::unexpected(value.error());
                               return complete_greeks([&](const PricingContext& shifted) { return fixed.price(option, shifted); },
                                                      option, context, greeks, settings, *make_pricing_result(*value));
                           });
}

} // namespace kiyosi::detail
