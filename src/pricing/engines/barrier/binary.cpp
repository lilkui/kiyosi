#include <kiyosi/pricing/engines/barrier/binary.hpp>
#include "../../detail/common.hpp"
#include <algorithm>
#include <cmath>

namespace kiyosi {
using namespace detail;
namespace {
struct Factors { double a1, b1, a2, b2, a3, b3, a4, b4, a5; };

double vanilla_digital(const BinaryBarrierOption& option, const PricingContext& context, double time)
{
    const double spot = context.asset_price().value(), rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield(), volatility = context.parameters().volatility();
    if (!option.type()) return option.asset_settlement() ? spot * std::exp(-dividend * time) : option.payout() * std::exp(-rate * time);
    const double sign = *option.type() == option_type::call ? 1.0 : -1.0;
    const double volatility_time = volatility * std::sqrt(time);
    const double d1 = (std::log(spot / option.strike()) + (rate - dividend + .5 * volatility * volatility) * time) / volatility_time;
    const double d2 = d1 - volatility_time;
    return option.asset_settlement() ? spot * std::exp(-dividend * time) * normal_cdf(sign * d1)
                                      : option.payout() * std::exp(-rate * time) * normal_cdf(sign * d2);
}

double terminal_payoff(const BinaryBarrierOption& option, double spot, bool observed)
{
    const bool upper = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out;
    const bool in_money = !option.type() || (*option.type() == option_type::call ? spot > option.strike() : spot < option.strike());
    const bool hit = observed && (upper ? spot >= option.barrier() : spot <= option.barrier());
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::down_and_in;
    return knock_in == hit && in_money ? (option.asset_settlement() ? spot : option.payout()) : 0.0;
}
}

result<PricingResult> AnalyticBinaryBarrierEngine::price(const BinaryBarrierOption& option, const PricingContext& context) const
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(Error{error_category::invalid_schedule, schedule.error().message});
    }
    const double time = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price().value();
    const bool upper = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out;
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::down_and_in;
    const bool observed_now = option.observation() == observation_mode::continuous ||
        std::ranges::any_of(option.observation_dates(), [&](date event) {
            return event == context.valuation_time();
        });
    const bool touched = observed_now && (upper ? spot >= option.barrier() : spot <= option.barrier());
    if (time == 0.0) return PricingResult{{risk_measure::price, terminal_payoff(option, spot, observed_now)}};
    if (touched) {
        if (!knock_in) return PricingResult{{risk_measure::price, 0.0}};
        if (option.settlement_timing() == rebate_timing::at_hit)
            return PricingResult{{risk_measure::price, option.asset_settlement() ? option.barrier() : option.payout()}};
        return PricingResult{{risk_measure::price, vanilla_digital(option, context, time)}};
    }
    const double rate = context.parameters().risk_free_rate(), dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility(), volatility_time = volatility * std::sqrt(time);
    double barrier = option.barrier();
    if (option.observation() == observation_mode::scheduled)
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * volatility * std::sqrt(option.observation_interval()));
    const double mu = (rate - dividend - .5 * volatility * volatility) / (volatility * volatility);
    const double lambda = std::sqrt(mu * mu + 2.0 * rate / (volatility * volatility));
    const double x1 = std::log(spot / option.strike()) / volatility_time + (1 + mu) * volatility_time;
    const double x2 = std::log(spot / barrier) / volatility_time + (1 + mu) * volatility_time;
    const double y1 = std::log(barrier * barrier / (spot * option.strike())) / volatility_time + (1 + mu) * volatility_time;
    const double y2 = std::log(barrier / spot) / volatility_time + (1 + mu) * volatility_time;
    const double z = std::log(barrier / spot) / volatility_time + lambda * volatility_time;
    const auto common = [&](double eta, double phi) {
        const double rate_discount = std::exp(-rate * time), dividend_discount = std::exp(-dividend * time), ratio = barrier / spot;
        return Factors{
            spot * dividend_discount * normal_cdf(phi * x1), option.payout() * rate_discount * normal_cdf(phi * x1 - phi * volatility_time),
            spot * dividend_discount * normal_cdf(phi * x2), option.payout() * rate_discount * normal_cdf(phi * x2 - phi * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y1), option.payout() * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y1 - eta * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y2), option.payout() * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y2 - eta * volatility_time),
            option.payout() * (std::pow(ratio, mu + lambda) * normal_cdf(eta * z) + std::pow(ratio, mu - lambda) * normal_cdf(eta * z - 2 * eta * lambda * volatility_time))};
    };
    if (option.settlement_timing() == rebate_timing::at_hit) {
        const auto factors = common(upper ? -1.0 : 1.0, 0.0);
        return PricingResult{{risk_measure::price, factors.a5}};
    }
    const bool down = !upper, call = option.type() && *option.type() == option_type::call;
    const double phi = option.type() ? (call ? 1.0 : -1.0)
                                     : (knock_in ? (down ? -1.0 : 1.0) : (down ? 1.0 : -1.0));
    const auto factors = common(down ? 1.0 : -1.0, phi);
    double value = 0.0;
    if (!option.type()) {
        value = option.asset_settlement()
                    ? (knock_in ? factors.a2 + factors.a4 : factors.a2 - factors.a4)
                    : (knock_in ? factors.b2 + factors.b4 : factors.b2 - factors.b4);
    } else if (knock_in && !option.asset_settlement()) {
        if (call) value = down ? (option.strike() > barrier ? factors.b3 : factors.b1 - factors.b2 + factors.b4)
                               : (option.strike() > barrier ? factors.b1 : factors.b2 - factors.b3 + factors.b4);
        else value = down ? (option.strike() > barrier ? factors.b2 - factors.b3 + factors.b4 : factors.b1)
                          : (option.strike() > barrier ? factors.b1 - factors.b2 + factors.b4 : factors.b3);
    } else if (knock_in) {
        if (call) value = down ? (option.strike() > barrier ? factors.a3 : factors.a1 - factors.a2 + factors.a4)
                               : (option.strike() > barrier ? factors.a1 : factors.a2 - factors.a3 + factors.a4);
        else value = down ? (option.strike() > barrier ? factors.a2 - factors.a3 + factors.a4 : factors.a1)
                          : (option.strike() > barrier ? factors.a1 - factors.a2 + factors.a4 : factors.a3);
    } else if (!option.asset_settlement()) {
        if (call) value = down ? (option.strike() > barrier ? factors.b1 - factors.b3 : factors.b2 - factors.b4)
                               : (option.strike() > barrier ? 0.0 : factors.b1 - factors.b2 + factors.b3 - factors.b4);
        else value = down ? (option.strike() > barrier ? factors.b1 - factors.b2 + factors.b3 - factors.b4 : 0.0)
                          : (option.strike() > barrier ? factors.b2 - factors.b4 : factors.b1 - factors.b3);
    } else {
        if (call) value = down ? (option.strike() > barrier ? factors.a1 - factors.a3 : factors.a2 - factors.a4)
                               : (option.strike() > barrier ? 0.0 : factors.a1 - factors.a2 + factors.a3 - factors.a4);
        else value = down ? (option.strike() > barrier ? factors.a1 - factors.a2 + factors.a3 - factors.a4 : 0.0)
                          : (option.strike() > barrier ? factors.a2 - factors.a4 : factors.a1 - factors.a3);
    }
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "binary barrier pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}
} // namespace kiyosi
