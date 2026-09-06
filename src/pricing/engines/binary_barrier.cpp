#include <kiyosi/pricing/engines/binary_barrier.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <functional>
#include <numbers>

namespace kiyosi {
using namespace detail;
namespace {
double discounted_hit_probability(double distance, double drift, double variance, double rate, double maturity)
{
    if (distance <= 0.0) return 1.0;
    constexpr int panels = 1024;
    const double step = maturity / panels;
    const double scale = distance / std::sqrt(2.0 * std::numbers::pi * variance);
    auto density = [&](double time) {
        if (time <= 0.0) return 0.0;
        return scale * std::exp(-((distance - drift * time) * (distance - drift * time)) /
                                (2.0 * variance * time)) /
               std::pow(time, 1.5) * std::exp(-rate * time);
    };
    double sum = density(maturity);
    for (int index = 1; index < panels; ++index)
        sum += (index % 2 ? 4.0 : 2.0) * density(index * step);
    return std::clamp(sum * step / 3.0, 0.0, 1.0);
}

double survival_density(double y, double boundary, bool upper, double drift, double variance, double t)
{
    if ((upper && y >= boundary) || (!upper && y <= boundary)) return 0.0;
    const double sd = std::sqrt(variance * t);
    const auto normal = [&](double x) { return std::exp(-0.5 * x * x) / (sd * std::sqrt(2.0 * std::numbers::pi)); };
    const double mean = drift * t;
    const double reflection = std::exp(-2.0 * (upper ? -drift : drift) * (upper ? boundary : -boundary) / variance);
    return upper ? normal(y - mean) - reflection * normal(y - 2.0 * boundary - mean)
                 : normal(y - mean) - reflection * normal(y - 2.0 * boundary - mean);
}
double integrate(const std::function<double(double)>& f, double a, double b)
{
    if (b <= a) return 0.0;
    constexpr int panels = 1024;
    const double h = (b - a) / panels;
    double sum = f(a) + f(b);
    for (int i = 1; i < panels; ++i)
        sum += (i % 2 ? 4.0 : 2.0) * f(a + i * h);
    return sum * h / 3.0;
}
} // namespace

result<PricingResult> AnalyticBinaryBarrierEngine::price(
    const BinaryBarrierOption& option, const PricingContext& context) const
{
    auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    const double t = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const bool upper = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out;
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::down_and_in;
    const bool touched = upper ? spot >= option.barrier() : spot <= option.barrier();
    if (sigma < 1e-12) {
        const double terminal = spot * std::exp((rate - dividend) * t);
        const bool hit = upper ? terminal >= option.barrier() || touched : terminal <= option.barrier() || touched;
        const bool in_money = !option.type() || (*option.type() == option_type::call ? terminal > option.strike() : terminal < option.strike());
        const double total = in_money ? (option.asset_settlement() ? terminal * std::exp(-rate * t) : option.payout() * std::exp(-rate * t)) : 0.0;
        const double value = (knock_in ? hit : !hit) ? total : 0.0;
        return PricingResult{{risk_measure::price, value}};
    }
    auto terminal_payoff = [&](double terminal) {
        const bool in_money = !option.type() || (*option.type() == option_type::call ? terminal > option.strike() : terminal < option.strike());
        return in_money ? (option.asset_settlement() ? terminal : option.payout()) : 0.0;
    };
    if (t == 0.0) {
        const double value = (knock_in ? touched : !touched) ? terminal_payoff(spot) : 0.0;
        return PricingResult{{risk_measure::price, value}};
    }
    const double drift = rate - dividend - 0.5 * sigma * sigma;
    const double variance = sigma * sigma;
    if (option.settlement_timing() == rebate_timing::at_hit && knock_in) {
        const double distance = upper ? std::log(option.barrier() / spot) : std::log(spot / option.barrier());
        const double hit = touched ? 1.0 : discounted_hit_probability(distance, upper ? drift : -drift,
                                                                        variance, rate, t);
        const double settlement = option.asset_settlement() ? option.barrier() : option.payout();
        const double value = settlement * hit;
        if (!std::isfinite(value))
            return std::unexpected(Error{error_category::invalid_result, "binary barrier pricing produced a non-finite result"});
        return PricingResult{{risk_measure::price, value}};
    }
    const double boundary = std::log(option.barrier() / spot);
    const double sd = sigma * std::sqrt(t);
    const double mean = drift * t;
    const double lower = upper ? mean - 12.0 * sd : std::max(mean - 12.0 * sd, boundary);
    const double higher = upper ? std::min(mean + 12.0 * sd, boundary) : mean + 12.0 * sd;
    double survival = 0.0;
    if (!touched) survival = std::exp(-rate * t) * integrate([&](double y) {
                                 const double terminal = spot * std::exp(y);
                                 return terminal_payoff(terminal) * survival_density(y, boundary, upper, drift, variance, t);
                             },
                                                             lower, higher);
    double total;
    if (!option.type()) {
        total = option.asset_settlement() ? spot * std::exp(-dividend * t) : option.payout() * std::exp(-rate * t);
    } else {
        if (option.asset_settlement()) total = spot * std::exp(-dividend * t) * normal_cdf((*option.type() == option_type::call ? 1.0 : -1.0) * ((std::log(spot / option.strike()) + (rate - dividend + 0.5 * sigma * sigma) * t) / (sigma * std::sqrt(t))));
        else total = option.payout() * std::exp(-rate * t) * normal_cdf((*option.type() == option_type::call ? 1.0 : -1.0) * ((std::log(spot / option.strike()) + (rate - dividend - 0.5 * sigma * sigma) * t) / (sigma * std::sqrt(t))));
    }
    const double value = knock_in ? total - survival : survival;
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "binary barrier pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}
} // namespace kiyosi
