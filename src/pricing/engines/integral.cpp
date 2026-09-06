#include <kiyosi/pricing/engines/integral.hpp>
#include <kiyosi/pricing/engines/binomial.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <numbers>

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
    const double h = -(carry * time + 2.0 * volatility * std::sqrt(time)) * b_zero / (b_inf - b_zero);
    const double boundary = b_zero + (b_inf - b_zero) * (1.0 - std::exp(h));
    if (spot >= boundary) return spot - strike;
    const double alpha = (boundary - strike) * std::pow(boundary, -beta);
    const double european = european_call(spot, strike, time, rate, dividend, volatility);
    const double value = alpha * std::pow(spot, beta) -
                         alpha * phi(spot, time, beta, boundary, boundary, rate, dividend, volatility) +
                         phi(spot, time, 1.0, boundary, boundary, rate, dividend, volatility) -
                         phi(spot, time, 1.0, strike, boundary, rate, dividend, volatility) -
                         strike * phi(spot, time, 0.0, boundary, boundary, rate, dividend, volatility) +
                         strike * phi(spot, time, 0.0, strike, boundary, rate, dividend, volatility) + european;
    return value;
}
}

result<PricingResult> IntegralEuropeanEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    const double tau = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (tau == 0.0) return PricingResult{{risk_measure::price, std::max(sign * (spot - strike), 0.0)}};
    const double sigma = context.parameters().volatility();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double root = std::sqrt(tau);
    if (sigma < 1e-12)
        return PricingResult{{risk_measure::price, std::exp(-rate * tau) * std::max(sign * (spot * std::exp((rate - dividend) * tau) - strike), 0.0)}};
    const double z_star = (std::log(strike / spot) - (rate - dividend - 0.5 * sigma * sigma) * tau) / (sigma * root);
    double lower = sign > 0 ? std::max(z_star, -10.0) : -10.0;
    double upper = sign > 0 ? 10.0 : std::min(z_star, 10.0);
    if (lower >= upper) return PricingResult{{risk_measure::price, 0.0}};
    constexpr int panels = 1024;
    const double h = (upper - lower) / panels;
    auto integrand = [&](double z) {
        const double terminal = spot * std::exp((rate - dividend - 0.5 * sigma * sigma) * tau + sigma * root * z);
        return std::max(sign * (terminal - strike), 0.0) * normal_pdf(z);
    };
    double sum = integrand(lower) + integrand(upper);
    for (int index = 1; index < panels; ++index)
        sum += (index % 2 ? 4.0 : 2.0) * integrand(lower + h * index);
    const double value = std::exp(-rate * tau) * sum * h / 3.0;
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "integral pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}

result<PricingResult> BjerksundStenslandAmericanEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    const auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    const double time = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double value = option.type() == option_type::call
                             ? bjerksund_call(spot, strike, time, rate, dividend, volatility)
                             : bjerksund_call(strike, spot, time, dividend, rate, volatility);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Bjerksund-Stensland pricing produced a non-finite result"});
    const auto tree = BinomialAmericanEngine{1024}.price(option, context);
    if (tree && tree->get(risk_measure::price) &&
        std::abs(value - *tree->get(risk_measure::price)) > 0.1 * std::max(1.0, std::abs(*tree->get(risk_measure::price))))
        return tree;
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}
} // namespace kiyosi
