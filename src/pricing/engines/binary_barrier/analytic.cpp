#include <kiyosi/pricing/engines/binary_barrier/analytic.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <variant>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
struct BinaryBarrierFormulaTerms {
    double a1, b1, a2, b2, a3, b3, a4, b4, a5;
};

struct BinaryBarrierContractView {
    const BarrierTerms& barrier_terms;
    std::optional<OptionType> option_type;
    double strike;
    double payout;
    bool asset_settlement;
    kiyosi::SettlementTiming settlement_timing;
};

BinaryBarrierContractView make_contract_view(const BinaryBarrierOption& option)
{
    const bool asset = option.payoff_type() == PayoffType::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {option.barrier_terms(), option.option_type(), option.strike(),
            cash ? cash->payout() : option.barrier_level(), asset, SettlementTiming::at_expiry};
}

BinaryBarrierContractView make_contract_view(const TouchOption& option)
{
    const bool asset = option.payoff_type() == PayoffType::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {option.barrier_terms(), std::nullopt, option.barrier_level(),
            cash ? cash->payout() : option.barrier_level(), asset, option.settlement_timing()};
}

double vanilla_digital(const BinaryBarrierContractView& option, const PricingContext& context, double time)
{
    const double spot = context.spot_price(), rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield(), volatility = context.model_parameters().volatility();
    if (!option.option_type) return option.asset_settlement ? spot * std::exp(-dividend * time) : option.payout * std::exp(-rate * time);
    const double sign = *option.option_type == OptionType::call ? 1.0 : -1.0;
    const double volatility_time = volatility * std::sqrt(time);
    const double d1 = (std::log(spot / option.strike) + (rate - dividend + .5 * volatility * volatility) * time) / volatility_time;
    const double d2 = d1 - volatility_time;
    return option.asset_settlement ? spot * std::exp(-dividend * time) * normal_cdf(sign * d1)
                                   : option.payout * std::exp(-rate * time) * normal_cdf(sign * d2);
}

double terminal_payoff(const BinaryBarrierContractView& option, double spot, bool observed)
{
    const auto& terms = option.barrier_terms;
    const bool in_money = !option.option_type || (*option.option_type == OptionType::call ? spot > option.strike : spot < option.strike);
    const bool hit = observed && terms.is_breached_by(spot);
    return terms.is_knock_in() == hit && in_money ? (option.asset_settlement ? spot : option.payout) : 0.0;
}

Result<PricingResult> price_contract(const BinaryBarrierContractView& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms;
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), terms.effective_date(), terms.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (terms.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(terms.observation_dates(), terms.effective_date(), terms.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(Error{ErrorCategory::invalid_schedule, schedule.error().message});
    }
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), terms.expiry_date());
    const double spot = context.spot_price();
    const bool upper = terms.is_up();
    const bool knock_in = terms.is_knock_in();
    const bool observed_now = terms.is_monitored_on(date_of(context.valuation_time()));
    const bool touched = observed_now && terms.is_breached_by(spot);
    if (time == 0.0)
        return make_pricing_result(
            {{RiskMeasure::price, terminal_payoff(option, spot, observed_now)}});
    if (touched) {
        if (!knock_in) return make_pricing_result({{RiskMeasure::price, 0.0}});
        if (option.settlement_timing == SettlementTiming::at_hit)
            return make_pricing_result(
                {{RiskMeasure::price,
                  option.asset_settlement ? terms.barrier_level() : option.payout}});
        return make_pricing_result(
            {{RiskMeasure::price, vanilla_digital(option, context, time)}});
    }
    const double rate = context.model_parameters().risk_free_rate(), dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility(), volatility_time = volatility * std::sqrt(time);
    double barrier = terms.barrier_level();
    if (terms.observation_mode() == ObservationMode::scheduled)
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * volatility * std::sqrt(terms.mean_observation_year_fraction()));
    const double mu = (rate - dividend - .5 * volatility * volatility) / (volatility * volatility);
    const double lambda = std::sqrt(mu * mu + 2.0 * rate / (volatility * volatility));
    const double x1 = std::log(spot / option.strike) / volatility_time + (1 + mu) * volatility_time;
    const double x2 = std::log(spot / barrier) / volatility_time + (1 + mu) * volatility_time;
    const double y1 = std::log(barrier * barrier / (spot * option.strike)) / volatility_time + (1 + mu) * volatility_time;
    const double y2 = std::log(barrier / spot) / volatility_time + (1 + mu) * volatility_time;
    const double z = std::log(barrier / spot) / volatility_time + lambda * volatility_time;
    const auto common = [&](double eta, double phi) {
        const double rate_discount = std::exp(-rate * time), dividend_discount = std::exp(-dividend * time), ratio = barrier / spot;
        return BinaryBarrierFormulaTerms{
            spot * dividend_discount * normal_cdf(phi * x1), option.payout * rate_discount * normal_cdf(phi * x1 - phi * volatility_time),
            spot * dividend_discount * normal_cdf(phi * x2), option.payout * rate_discount * normal_cdf(phi * x2 - phi * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y1), option.payout * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y1 - eta * volatility_time),
            spot * dividend_discount * std::pow(ratio, 2 * (mu + 1)) * normal_cdf(eta * y2), option.payout * rate_discount * std::pow(ratio, 2 * mu) * normal_cdf(eta * y2 - eta * volatility_time),
            option.payout * (std::pow(ratio, mu + lambda) * normal_cdf(eta * z) + std::pow(ratio, mu - lambda) * normal_cdf(eta * z - 2 * eta * lambda * volatility_time))};
    };
    if (option.settlement_timing == SettlementTiming::at_hit) {
        const auto formula_terms = common(upper ? -1.0 : 1.0, 0.0);
        return make_pricing_result({{RiskMeasure::price, formula_terms.a5}});
    }
    const bool down = !upper, call = option.option_type && *option.option_type == OptionType::call;
    const double phi = option.option_type ? (call ? 1.0 : -1.0)
                                          : (knock_in ? (down ? -1.0 : 1.0) : (down ? 1.0 : -1.0));
    const auto formula_terms = common(down ? 1.0 : -1.0, phi);
    double value = 0.0;
    if (!option.option_type) {
        value = option.asset_settlement
                    ? (knock_in ? formula_terms.a2 + formula_terms.a4 : formula_terms.a2 - formula_terms.a4)
                    : (knock_in ? formula_terms.b2 + formula_terms.b4 : formula_terms.b2 - formula_terms.b4);
    } else if (knock_in && !option.asset_settlement) {
        if (call) value = down ? (option.strike > barrier ? formula_terms.b3 : formula_terms.b1 - formula_terms.b2 + formula_terms.b4)
                               : (option.strike > barrier ? formula_terms.b1 : formula_terms.b2 - formula_terms.b3 + formula_terms.b4);
        else value = down ? (option.strike > barrier ? formula_terms.b2 - formula_terms.b3 + formula_terms.b4 : formula_terms.b1)
                          : (option.strike > barrier ? formula_terms.b1 - formula_terms.b2 + formula_terms.b4 : formula_terms.b3);
    } else if (knock_in) {
        if (call) value = down ? (option.strike > barrier ? formula_terms.a3 : formula_terms.a1 - formula_terms.a2 + formula_terms.a4)
                               : (option.strike > barrier ? formula_terms.a1 : formula_terms.a2 - formula_terms.a3 + formula_terms.a4);
        else value = down ? (option.strike > barrier ? formula_terms.a2 - formula_terms.a3 + formula_terms.a4 : formula_terms.a1)
                          : (option.strike > barrier ? formula_terms.a1 - formula_terms.a2 + formula_terms.a4 : formula_terms.a3);
    } else if (!option.asset_settlement) {
        if (call) value = down ? (option.strike > barrier ? formula_terms.b1 - formula_terms.b3 : formula_terms.b2 - formula_terms.b4)
                               : (option.strike > barrier ? 0.0 : formula_terms.b1 - formula_terms.b2 + formula_terms.b3 - formula_terms.b4);
        else value = down ? (option.strike > barrier ? formula_terms.b1 - formula_terms.b2 + formula_terms.b3 - formula_terms.b4 : 0.0)
                          : (option.strike > barrier ? formula_terms.b2 - formula_terms.b4 : formula_terms.b1 - formula_terms.b3);
    } else {
        if (call) value = down ? (option.strike > barrier ? formula_terms.a1 - formula_terms.a3 : formula_terms.a2 - formula_terms.a4)
                               : (option.strike > barrier ? 0.0 : formula_terms.a1 - formula_terms.a2 + formula_terms.a3 - formula_terms.a4);
        else value = down ? (option.strike > barrier ? formula_terms.a1 - formula_terms.a2 + formula_terms.a3 - formula_terms.a4 : 0.0)
                          : (option.strike > barrier ? formula_terms.a2 - formula_terms.a4 : formula_terms.a1 - formula_terms.a3);
    }
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "binary barrier pricing produced a non-finite result"});
    return make_pricing_result({{RiskMeasure::price, std::max(value, 0.0)}});
}
} // namespace

Result<PricingResult> AnalyticBinaryBarrierEngine::price_native(
    const BinaryBarrierOption& option, const PricingContext& context) const
{
    return price_contract(make_contract_view(option), context);
}

Result<PricingResult> AnalyticBinaryBarrierEngine::price_native(
    const TouchOption& option, const PricingContext& context) const
{
    return price_contract(make_contract_view(option), context);
}
} // namespace kiyosi
