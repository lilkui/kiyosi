#pragma once

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <optional>
#include <ratio>
#include <span>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/accumulator_traits.hpp>
#include <kiyosi/pricing/detail/asian_traits.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>
#include <kiyosi/pricing/detail/barrier_traits.hpp>
#include <kiyosi/pricing/detail/finite_difference_domain.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

namespace detail {

template <typename Engine, typename Option>
[[nodiscard]] Result<double> numerical_value(
    const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    return checked_price(*priced);
}

[[nodiscard]] inline Result<PricingContext> shifted_context(
    const PricingContext& context, double spot, double volatility, double rate, Timestamp valuation)
{
    auto parameters = make_bsm_parameters(rate, context.model_parameters().dividend_yield(), volatility);
    if (!parameters) return std::unexpected(parameters.error());
    return make_pricing_context(*parameters, spot, valuation, context.calendar());
}

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

/// Extracts a checked scalar from the private native engine result.
inline Result<double> price_value(const Result<PricingResult>& result)
{
    if (!result) return std::unexpected(result.error());
    return checked_price(result->price());
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

// Use the rounded upper bump on both sides; a collapsed or asymmetric stencil is unavailable.
inline double symmetric_shift(double value, double requested)
{
    const double high = value + requested;
    const double shift = high - value;
    const double low = value - shift;
    return std::isfinite(high) && std::isfinite(shift) && shift > 0.0 &&
                   std::isfinite(low) && value - low == shift
               ? shift
               : 0.0;
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
    const double r = symmetric_shift(context.model_parameters().risk_free_rate(), settings.rate_shift);
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

// Enforces boundary availability, then fills only missing requested measures.
// Away from unavailable boundaries, a supplied native value is never overwritten.
template <typename Price, typename Option, typename Native = decltype(nullptr), typename SpotBatch = decltype(nullptr)>
Result<PricingResult> complete_greeks(
    const Price& price, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const PricingResult& native,
    const Native& evaluate_native = nullptr, const SpotBatch& evaluate_spot_batch = nullptr)
{
    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation_time = context.valuation_time();
    const Result<double> p0 = native.price();
    if (greeks_unavailable(option, context))
        return make_pricing_result(*p0);
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
        const auto value = price_value(result);
        if (!value) return std::unexpected(value.error());
        if (!result->all_finite())
            return std::unexpected(Error{ErrorCategory::invalid_result, "native Greeks are non-finite"});
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
        std::optional<PricingResult> native_up, native_down;
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
                native_up = *high;
                native_down = *low;
            }
        }
        const auto p_up = native_up ? Result<double>{native_up->price()} : detail::shifted_value(price, context, spot + h, volatility, rate, valuation_time);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = native_down ? Result<double>{native_down->price()} : detail::shifted_value(price, context, spot - h, volatility, rate, valuation_time);
        if (!p_down) return std::unexpected(p_down.error());
        spot_up = *p_up;
        spot_down = *p_down;
        if (need(Greek::delta)) delta = (*p_up - *p_down) / (2.0 * h);
        // Divide separately so powers of the shift cannot overflow or underflow.
        if (need(Greek::gamma)) gamma = (*p_up - 2.0 * *p0 + *p_down) / h / h;

        const bool native_speed = native_up && native_up->has(Greek::gamma) && native_down->has(Greek::gamma);
        if (need(Greek::speed) && native_speed)
            speed = (**native_up->get(Greek::gamma) - **native_down->get(Greek::gamma)) / (2.0 * h);

        if (need(Greek::speed) && !native_speed && scalar_speed_available) {
            const auto p_up2 = detail::shifted_value(
                price, context, spot + two_h, volatility, rate, valuation_time);
            if (!p_up2) return std::unexpected(p_up2.error());
            const auto p_down2 = detail::shifted_value(
                price, context, spot - two_h, volatility, rate, valuation_time);
            if (!p_down2) return std::unexpected(p_down2.error());
            speed = (*p_up2 - 2.0 * *p_up + 2.0 * *p_down - *p_down2) /
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
        std::optional<PricingResult> native_high, native_low;
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
                native_high = *high;
                native_low = *low;
            }
        }
        const bool native_vanna = native_high && native_high->has(Greek::delta) && native_low->has(Greek::delta);
        const bool native_zomma = native_high && native_high->has(Greek::gamma) && native_low->has(Greek::gamma);
        const bool scalar_mixed = spot_stencil_available && ((need(Greek::vanna) && !native_vanna) || (need(Greek::zomma) && !native_zomma));
        const bool need_volatility_prices = need(Greek::vega) || need(Greek::zomma);
        std::optional<std::array<NumericalSpotPrices, 2>> batches;
        if constexpr (requires { evaluate_spot_batch(context, h, need_volatility_prices); }) {
            if (scalar_mixed) {
                const auto sample = [&](double shifted_volatility) -> Result<NumericalSpotPrices> {
                    const auto shifted = shifted_context(context, spot, shifted_volatility, rate, valuation_time);
                    if (!shifted) return std::unexpected(shifted.error());
                    const auto prices = evaluate_spot_batch(*shifted, h, need_volatility_prices);
                    if (!prices) return std::unexpected(prices.error());
                    if ((need_volatility_prices && !prices->center) || !std::isfinite(prices->up) ||
                        !std::isfinite(prices->down) || (prices->center && !std::isfinite(*prices->center)))
                        return std::unexpected(Error{ErrorCategory::invalid_result, "numerical spot batch is incomplete or non-finite"});
                    return prices;
                };
                const auto high = sample(volatility_high);
                if (!high) return std::unexpected(high.error());
                const auto low = sample(volatility_low);
                if (!low) return std::unexpected(low.error());
                batches = std::array{*high, *low};
            }
        }
        const auto v_up = native_high ? Result<double>{native_high->price()} : batches ? Result<double>{(*batches)[0].center.value_or(*p0)} : need_volatility_prices ? detail::shifted_value(price, context, spot, volatility_high, rate, valuation_time)
                                                                                                      : p0;
        if (!v_up) return std::unexpected(v_up.error());
        const auto d_up = batches ? Result<double>{(*batches)[0].up} : scalar_mixed ? detail::shifted_value(price, context, spot + h, volatility_high, rate, valuation_time) : p0;
        if (!d_up) return std::unexpected(d_up.error());
        const auto d_down = batches ? Result<double>{(*batches)[0].down} : scalar_mixed ? detail::shifted_value(price, context, spot - h, volatility_high, rate, valuation_time) : p0;
        if (!d_down) return std::unexpected(d_down.error());
        const auto v_down = native_low ? Result<double>{native_low->price()} : batches ? Result<double>{(*batches)[1].center.value_or(*p0)} : need_volatility_prices ? detail::shifted_value(price, context, spot, volatility_low, rate, valuation_time)
                                                                                                      : p0;
        if (!v_down) return std::unexpected(v_down.error());
        if (need(Greek::vega)) vega = (*v_up - *v_down) / (2.0 * vol_scale);

        if (need(Greek::vanna) && native_vanna)
            vanna = (**native_high->get(Greek::delta) - **native_low->get(Greek::delta)) / (2.0 * vol_scale);
        if (need(Greek::zomma) && native_zomma)
            zomma = (**native_high->get(Greek::gamma) - **native_low->get(Greek::gamma)) / (2.0 * vol_scale);

        if (scalar_mixed) {
            const auto d_up_low = batches ? Result<double>{(*batches)[1].up} : detail::shifted_value(
                price, context, spot + h, volatility_low, rate, valuation_time);
            if (!d_up_low) return std::unexpected(d_up_low.error());
            const auto d_down_low = batches ? Result<double>{(*batches)[1].down} : detail::shifted_value(
                price, context, spot - h, volatility_low, rate, valuation_time);
            if (!d_down_low) return std::unexpected(d_down_low.error());
            if (need(Greek::vanna) && !native_vanna) vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) /
                                                             h / vol_scale / 4.0;

            if (need(Greek::zomma) && !native_zomma) {
                const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / h / h;
                const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / h / h;
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
        if constexpr (requires { option.effective_date(); })
            lower_bound = start_of_day(option.effective_date());
        if constexpr (requires { option.expiry_date(); })
            upper_bound = start_of_day(option.expiry_date());
        // Bound whole days before conversion so even INT_MAX shifts fit in microseconds.
        const auto requested_shift = std::chrono::duration_cast<Timestamp::duration>(
            std::min(std::chrono::days{settings.time_shift_days}, std::chrono::floor<std::chrono::days>(upper_bound - lower_bound)));
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
            std::optional<PricingResult> native_before, native_after;
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
                    native_before = *first;
                    native_after = *last;
                }
            }
            const bool native_charm = native_before && native_before->has(Greek::delta) && native_after->has(Greek::delta);
            const bool native_color = native_before && native_before->has(Greek::gamma) && native_after->has(Greek::gamma);
            const bool scalar_mixed = spot_stencil_available && ((need(Greek::charm) && !native_charm) || (need(Greek::color) && !native_color));
            const bool need_time_prices = need(Greek::theta) || (spot_stencil_available && need(Greek::color) && !native_color);
            const auto t_before = native_before ? Result<double>{native_before->price()} : !need_time_prices || before == valuation_time ? p0
                                                                                                                                         : detail::shifted_value(price, context, spot, volatility, rate, before);
            if (!t_before) return std::unexpected(t_before.error());
            const auto d_before = !scalar_mixed ? p0 : before == valuation_time && spot_up ? Result<double>{*spot_up}
                                                                                           : detail::shifted_value(price, context, spot + h, volatility, rate, before);
            if (!d_before) return std::unexpected(d_before.error());
            const auto d_before_low = !scalar_mixed ? p0 : before == valuation_time && spot_down ? Result<double>{*spot_down}
                                                                                                 : detail::shifted_value(price, context, spot - h, volatility, rate, before);
            if (!d_before_low) return std::unexpected(d_before_low.error());
            const auto t_after = native_after ? Result<double>{native_after->price()} : !need_time_prices || after == valuation_time ? p0
                                                                                                                                     : detail::shifted_value(price, context, spot, volatility, rate, after);
            if (!t_after) return std::unexpected(t_after.error());
            const double day_scale = Days{after - before}.count();
            if (need(Greek::theta)) theta = (*t_after - *t_before) / day_scale;
            if (need(Greek::charm) && native_charm)
                charm = (**native_after->get(Greek::delta) - **native_before->get(Greek::delta)) / day_scale;
            if (need(Greek::color) && native_color)
                color = (**native_after->get(Greek::gamma) - **native_before->get(Greek::gamma)) / day_scale;

            if (scalar_mixed) {
                const auto d_after = after == valuation_time && spot_up
                                         ? Result<double>{*spot_up}
                                         : detail::shifted_value(
                                               price, context, spot + h, volatility, rate, after);
                if (!d_after) return std::unexpected(d_after.error());
                const auto d_after_low = after == valuation_time && spot_down
                                             ? Result<double>{*spot_down}
                                             : detail::shifted_value(
                                                   price, context, spot - h, volatility, rate, after);
                if (!d_after_low) return std::unexpected(d_after_low.error());
                if (need(Greek::charm) && !native_charm) charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                                                                 h / day_scale / 2.0;
                if (need(Greek::color) && !native_color) color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                                                                   (*d_before - 2.0 * *t_before + *d_before_low)) /
                                                                  h / h / day_scale);
            }
        }
    }
    auto output = make_pricing_result(*p0, {{Greek::delta, delta}, {Greek::gamma, gamma}, {Greek::speed, speed}, {Greek::theta, theta}, {Greek::charm, charm}, {Greek::color, color}, {Greek::vega, vega}, {Greek::vanna, vanna}, {Greek::zomma, zomma}, {Greek::rho, rho}});
    if (!output) return std::unexpected(output.error());
    *output = output->selected(greeks);
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "numerical analytics are non-finite"});
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
            if (!grid.asset_upper_boundary) {
                // Spot bumps share one mesh; settlements may never construct a grid.
                const double upper = default_finite_difference_upper_boundary(option, context);
                if (std::isfinite(upper)) {
                    grid.asset_upper_boundary = upper;
                    return evaluate(Engine{grid});
                }
            }
        }
        return evaluate(seeded);
    });
    const auto value = price_value(result);
    if (!value) return std::unexpected(value.error());
    *result = result->selected(greeks);
    if (!result->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "native Greeks are non-finite"});
    if (greeks_unavailable(option, context)) return make_pricing_result(*value);
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
        const auto value = price_value(native);
        if (!value) return std::unexpected(value.error());
        *native = native->selected(native_request);
        if (!native->all_finite())
            return std::unexpected(Error{ErrorCategory::invalid_result, "native Greeks are non-finite"});
        const auto price = [&](const PricingContext& shifted) {
            if constexpr (is_builtin_finite_difference_engine<Engine, Option>)
                return price_value(evaluate_native(fixed, shifted, GreeksRequest{}));
            else
                return fixed.price(option, shifted);
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

} // namespace detail

/// Computes price and all feasible Greeks using only numerical price differences.
/// At expiry or a monitored barrier hit-state boundary, only price is available.
/// Missing legal stencils leave individual measures empty; a failed feasible valuation
/// fails the operation. Built-in Monte Carlo valuations share one seed per request.
/// Spot stencils crossing a currently monitored event threshold are unavailable.
/// Built-in finite-difference valuations share the unshifted context's asset domain.
/// Custom engines are used as supplied and control their own sampling and asset domain.
/// Shifts and units follow NumericalShiftSettings and Greek, respectively.
template <typename Engine, typename Option>
[[nodiscard]] Result<PricingResult> calculate_numerical_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    if constexpr (detail::is_builtin_monte_carlo_engine<Engine, Option>)
        return engine.price_with_greeks(option, context, true, settings);
    else
        return detail::price_with_greeks(engine, option, context, true, settings);
}

} // namespace kiyosi
