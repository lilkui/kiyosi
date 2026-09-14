#include <kiyosi/pricing/engines/vanilla/bjerksund_stensland.hpp>

#include <algorithm>
#include <cmath>
#include <numbers>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {
double european_call(double spot, double strike, double time, double rate, double dividend, double volatility)
{
    const double root = std::sqrt(time);
    const double d1 = (std::log(spot / strike) + (rate - dividend + 0.5 * volatility * volatility) * time) /
                      (volatility * root);
    const double d2 = d1 - volatility * root;
    return spot * std::exp(-dividend * time) * normal_cdf(d1) -
           strike * std::exp(-rate * time) * normal_cdf(d2);
}

double phi(double spot, double time, double gamma, double boundary, double strike_boundary,
           double rate, double dividend, double volatility)
{
    const double variance = volatility * volatility;
    const double root = volatility * std::sqrt(time);
    const double lambda = -rate + gamma * (rate - dividend) + 0.5 * gamma * (gamma - 1.0) * variance;
    const double kappa = 2.0 * (rate - dividend) / variance + 2.0 * gamma - 1.0;
    const double first = -(std::log(spot / boundary) +
                           (rate - dividend + (gamma - 0.5) * variance) * time) / root;
    const double second = -(std::log(strike_boundary * strike_boundary / (spot * boundary)) +
                            (rate - dividend + (gamma - 0.5) * variance) * time) / root;
    return std::exp(lambda * time) * std::pow(spot, gamma) *
           (normal_cdf(first) - std::pow(strike_boundary / spot, kappa) * normal_cdf(second));
}

double bivariate_normal_cdf(double first, double second, double correlation)
{
    constexpr double abscissas[] = {
        0.07652652113349733, 0.22778585114164508, 0.37370608871541956,
        0.5108670019508271, 0.636053680726515, 0.7463319064601508,
        0.8391169718222188, 0.9122344282513259, 0.9639719272779138, 0.9931285991850949};
    constexpr double weights[] = {
        0.15275338713072585, 0.14917298647260375, 0.14209610931838205,
        0.13168863844917663, 0.11819453196151842, 0.10193011981724044,
        0.08327674157670475, 0.06267204833410906, 0.04060142980038694, 0.01761400713915212};
    const double angle = std::asin(correlation);
    const double product = first * second;
    const double half_sum = 0.5 * (first * first + second * second);
    double integral = 0.0;
    for (int index = 0; index < 10; ++index) {
        for (const double sign : {-1.0, 1.0}) {
            const double sine = std::sin(angle * 0.5 * (1.0 + sign * abscissas[index]));
            integral += weights[index] * std::exp((sine * product - half_sum) / (1.0 - sine * sine));
        }
    }
    return normal_cdf(first) * normal_cdf(second) + angle * integral / (4.0 * std::numbers::pi);
}

double ksi(double spot, double time, double gamma, double boundary, double outer_boundary,
           double inner_boundary, double split_time, double rate, double carry, double volatility)
{
    const double variance = volatility * volatility;
    const double split_root = volatility * std::sqrt(split_time);
    const double root = volatility * std::sqrt(time);
    const double drift = carry + (gamma - 0.5) * variance;
    const double e1 = (std::log(spot / inner_boundary) + drift * split_time) / split_root;
    const double e2 = (std::log(outer_boundary * outer_boundary / (spot * inner_boundary)) +
                       drift * split_time) / split_root;
    const double e3 = (std::log(spot / inner_boundary) - drift * split_time) / split_root;
    const double e4 = (std::log(outer_boundary * outer_boundary / (spot * inner_boundary)) -
                       drift * split_time) / split_root;
    const double f1 = (std::log(spot / boundary) + drift * time) / root;
    const double f2 = (std::log(outer_boundary * outer_boundary / (spot * boundary)) + drift * time) / root;
    const double f3 = (std::log(inner_boundary * inner_boundary / (spot * boundary)) + drift * time) / root;
    const double f4 = (std::log(spot * inner_boundary * inner_boundary /
                              (boundary * outer_boundary * outer_boundary)) + drift * time) / root;
    const double correlation = std::sqrt(split_time / time);
    const double lambda = -rate + gamma * carry + 0.5 * gamma * (gamma - 1.0) * variance;
    const double kappa = 2.0 * carry / variance + 2.0 * gamma - 1.0;
    return std::exp(lambda * time) * std::pow(spot, gamma) *
           (bivariate_normal_cdf(-e1, -f1, correlation) -
            std::pow(outer_boundary / spot, kappa) * bivariate_normal_cdf(-e2, -f2, correlation) -
            std::pow(inner_boundary / spot, kappa) * bivariate_normal_cdf(-e3, -f3, -correlation) +
            std::pow(inner_boundary / outer_boundary, kappa) * bivariate_normal_cdf(-e4, -f4, -correlation));
}

double bjerksund_call(double spot, double strike, double time, double rate, double dividend, double volatility)
{
    if (time == 0.0) return std::max(spot - strike, 0.0);
    if (dividend <= 0.0) return european_call(spot, strike, time, rate, dividend, volatility);
    const double variance = volatility * volatility;
    const double radicand = std::pow((rate - dividend) / variance - 0.5, 2.0) + 2.0 * rate / variance;
    if (radicand <= 0.0 || !std::isfinite(radicand)) return european_call(spot, strike, time, rate, dividend, volatility);
    const double beta = (0.5 - (rate - dividend) / variance) + std::sqrt(radicand);
    if (!std::isfinite(beta) || beta <= 1.0) return european_call(spot, strike, time, rate, dividend, volatility);
    const double b_inf = beta / (beta - 1.0) * strike;
    const double carry = rate - dividend;
    const double b_zero = dividend > 0.0 ? std::max(strike, rate / dividend * strike) : strike;
    const double split_time = 0.5 * (std::sqrt(5.0) - 1.0) * time;
    const double scale = strike * strike / ((b_inf - b_zero) * b_zero);
    const double h1 = -(carry * split_time + 2.0 * volatility * std::sqrt(split_time)) * scale;
    const double h2 = -(carry * time + 2.0 * volatility * std::sqrt(time)) * scale;
    const double inner = b_zero + (b_inf - b_zero) * (1.0 - std::exp(h1));
    const double outer = b_zero + (b_inf - b_zero) * (1.0 - std::exp(h2));
    if (spot >= outer) return spot - strike;
    const double alpha1 = (inner - strike) * std::pow(inner, -beta);
    const double alpha2 = (outer - strike) * std::pow(outer, -beta);
    return alpha2 * std::pow(spot, beta) -
           alpha2 * phi(spot, split_time, beta, outer, outer, rate, dividend, volatility) +
           phi(spot, split_time, 1.0, outer, outer, rate, dividend, volatility) -
           phi(spot, split_time, 1.0, inner, outer, rate, dividend, volatility) -
           strike * phi(spot, split_time, 0.0, outer, outer, rate, dividend, volatility) +
           strike * phi(spot, split_time, 0.0, inner, outer, rate, dividend, volatility) +
           alpha1 * phi(spot, split_time, beta, inner, outer, rate, dividend, volatility) -
           alpha1 * ksi(spot, time, beta, inner, outer, inner, split_time, rate, carry, volatility) +
           ksi(spot, time, 1.0, inner, outer, inner, split_time, rate, carry, volatility) -
           ksi(spot, time, 1.0, strike, outer, inner, split_time, rate, carry, volatility) -
           strike * ksi(spot, time, 0.0, inner, outer, inner, split_time, rate, carry, volatility) +
           strike * ksi(spot, time, 0.0, strike, outer, inner, split_time, rate, carry, volatility);
}
}

result<PricingResult> BjerksundStenslandVanillaEngine::price_impl(const AmericanOption& option, const PricingContext& context) const
{
    const auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    const double time = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price();
    const double strike = option.strike();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double value = option.type() == option_type::call
                             ? bjerksund_call(spot, strike, time, rate, dividend, volatility)
                             : bjerksund_call(strike, spot, time, dividend, rate, volatility);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Bjerksund-Stensland pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}

} // namespace kiyosi
