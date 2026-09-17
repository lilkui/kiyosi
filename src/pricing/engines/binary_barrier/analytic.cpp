#include <kiyosi/pricing/engines/binary_barrier/analytic.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <variant>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
struct Factors { double a1, b1, a2, b2, a3, b3, a4, b4, a5; };

struct Contract {
    const BarrierTerms& barrier_terms;
    std::optional<option_type> type;
    double strike;
    double payout;
    bool asset_settlement;
    kiyosi::settlement_timing settlement_timing;
};

Contract contract(const BinaryBarrierOption& option)
{
    const bool asset = option.payoff_kind() == payoff_type::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {option.barrier_terms(), option.type(), option.strike(),
            cash ? cash->payout() : option.barrier(), asset, settlement_timing::at_expiry};
}

Contract contract(const TouchOption& option)
{
    const bool asset = option.payoff_kind() == payoff_type::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {option.barrier_terms(), std::nullopt, option.barrier(),
            cash ? cash->payout() : option.barrier(), asset, option.settlement_timing()};
}

double vanilla_digital(const Contract& option, const PricingContext& context, double time)
{
    const double spot = context.asset_price(), rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield(), volatility = context.parameters().volatility();
    if (!option.type) return option.asset_settlement ? spot * std::exp(-dividend * time) : option.payout * std::exp(-rate * time);
    const double sign = *option.type == option_type::call ? 1.0 : -1.0;
    const double volatility_time = volatility * std::sqrt(time);
    const double d1 = (std::log(spot / option.strike) + (rate - dividend + .5 * volatility * volatility) * time) / volatility_time;
    const double d2 = d1 - volatility_time;
    return option.asset_settlement ? spot * std::exp(-dividend * time) * normal_cdf(sign * d1)
                                   : option.payout * std::exp(-rate * time) * normal_cdf(sign * d2);
}

double terminal_payoff(const Contract& option, double spot, bool observed)
{
    const auto& terms = option.barrier_terms;
    const bool in_money = !option.type || (*option.type == option_type::call ? spot > option.strike : spot < option.strike);
    const bool hit = observed && terms.breaches(spot);
    return terms.is_knock_in() == hit && in_money ? (option.asset_settlement ? spot : option.payout) : 0.0;
}

result<PricingResult> price_contract(const Contract& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms;
    auto valid = validate_life(context.valuation_time(), terms.effective(), terms.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (terms.observation_mode() == observation_mode::scheduled) {
        auto schedule = validate_observation_dates(terms.observation_dates(), terms.effective(), terms.expiry(), context.calendar());
        if (!schedule) return std::unexpected(Error{error_category::invalid_schedule, schedule.error().message});
    }
    const double time = actual_365(context.valuation_time(), terms.expiry());
    const double spot = context.asset_price();
    const bool upper = terms.is_up();
    const bool knock_in = terms.is_knock_in();
    const bool observed_now = terms.monitors(context.valuation_time());
    const bool touched = observed_now && terms.breaches(spot);
    if (time == 0.0)
        return make_pricing_result(
            {{risk_measure::price, terminal_payoff(option, spot, observed_now)}});
    if (touched) {
        if (!knock_in) return make_pricing_result({{risk_measure::price, 0.0}});
        if (option.settlement_timing == settlement_timing::at_hit)
            return make_pricing_result(
                {{risk_measure::price,
                  option.asset_settlement ? terms.barrier() : option.payout}});
        return make_pricing_result(
            {{risk_measure::price, vanilla_digital(option, context, time)}});
    }
    const double rate = context.parameters().risk_free_rate(), dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility(), volatility_time = volatility * std::sqrt(time);
    double barrier = terms.barrier();
    if (terms.observation_mode() == observation_mode::scheduled)
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * volatility * std::sqrt(terms.observation_interval()));
    const double mu = (rate - dividend - .5 * volatility * volatility) / (volatility * volatility);
    const double lambda = std::sqrt(mu * mu + 2.0 * rate / (volatility * volatility));
    const double x1 = std::log(spot / option.strike) / volatility_time + (1 + mu) * volatility_time;
    const double x2 = std::log(spot / barrier) / volatility_time + (1 + mu) * volatility_time;
    const double y1 = std::log(barrier * barrier / (spot * option.strike)) / volatility_time + (1 + mu) * volatility_time;
    const double y2 = std::log(barrier / spot) / volatility_time + (1 + mu) * volatility_time;
    const double z = std::log(barrier / spot) / volatility_time + lambda * volatility_time;
    const auto common = [&](double eta, double phi) {
        const double rate_discount = std::exp(-rate * time), dividend_discount = std::exp(-dividend * time), ratio = barrier / spot;
        return Factors{
            spot * dividend_discount * normal_cdf(phi * x1), option.payout * rate_discount * normal_cdf(phi * x1 - phi * volatility_time),
            spot * dividend_discount * normal_cdf(phi * x2), option.payout * rate_discount * normal_cdf(phi * x2 - phi * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y1), option.payout * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y1 - eta * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y2), option.payout * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y2 - eta * volatility_time),
            option.payout * (std::pow(ratio, mu + lambda) * normal_cdf(eta * z) + std::pow(ratio, mu - lambda) * normal_cdf(eta * z - 2 * eta * lambda * volatility_time))};
    };
    if (option.settlement_timing == settlement_timing::at_hit) {
        const auto factors = common(upper ? -1.0 : 1.0, 0.0);
        return make_pricing_result({{risk_measure::price, factors.a5}});
    }
    const bool down = !upper, call = option.type && *option.type == option_type::call;
    const double phi = option.type ? (call ? 1.0 : -1.0)
                                   : (knock_in ? (down ? -1.0 : 1.0) : (down ? 1.0 : -1.0));
    const auto factors = common(down ? 1.0 : -1.0, phi);
    double value = 0.0;
    if (!option.type) {
        value = option.asset_settlement
                    ? (knock_in ? factors.a2 + factors.a4 : factors.a2 - factors.a4)
                    : (knock_in ? factors.b2 + factors.b4 : factors.b2 - factors.b4);
    } else if (knock_in && !option.asset_settlement) {
        if (call) value = down ? (option.strike > barrier ? factors.b3 : factors.b1 - factors.b2 + factors.b4)
                               : (option.strike > barrier ? factors.b1 : factors.b2 - factors.b3 + factors.b4);
        else value = down ? (option.strike > barrier ? factors.b2 - factors.b3 + factors.b4 : factors.b1)
                          : (option.strike > barrier ? factors.b1 - factors.b2 + factors.b4 : factors.b3);
    } else if (knock_in) {
        if (call) value = down ? (option.strike > barrier ? factors.a3 : factors.a1 - factors.a2 + factors.a4)
                               : (option.strike > barrier ? factors.a1 : factors.a2 - factors.a3 + factors.a4);
        else value = down ? (option.strike > barrier ? factors.a2 - factors.a3 + factors.a4 : factors.a1)
                          : (option.strike > barrier ? factors.a1 - factors.a2 + factors.a4 : factors.a3);
    } else if (!option.asset_settlement) {
        if (call) value = down ? (option.strike > barrier ? factors.b1 - factors.b3 : factors.b2 - factors.b4)
                               : (option.strike > barrier ? 0.0 : factors.b1 - factors.b2 + factors.b3 - factors.b4);
        else value = down ? (option.strike > barrier ? factors.b1 - factors.b2 + factors.b3 - factors.b4 : 0.0)
                          : (option.strike > barrier ? factors.b2 - factors.b4 : factors.b1 - factors.b3);
    } else {
        if (call) value = down ? (option.strike > barrier ? factors.a1 - factors.a3 : factors.a2 - factors.a4)
                               : (option.strike > barrier ? 0.0 : factors.a1 - factors.a2 + factors.a3 - factors.a4);
        else value = down ? (option.strike > barrier ? factors.a1 - factors.a2 + factors.a3 - factors.a4 : 0.0)
                          : (option.strike > barrier ? factors.a2 - factors.a4 : factors.a1 - factors.a3);
    }
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "binary barrier pricing produced a non-finite result"});
    return make_pricing_result({{risk_measure::price, std::max(value, 0.0)}});
}
} // namespace

result<PricingResult> AnalyticBinaryBarrierEngine::price(
    const BinaryBarrierOption& option, const PricingContext& context) const
{
    return price_contract(contract(option), context);
}

result<PricingResult> AnalyticBinaryBarrierEngine::price(
    const TouchOption& option, const PricingContext& context) const
{
    return price_contract(contract(option), context);
}
} // namespace kiyosi
