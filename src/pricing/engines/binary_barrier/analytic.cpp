#include <kiyosi/pricing/engines/binary_barrier/analytic.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/engines/digital/analytic.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <variant>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
struct BinaryBarrierContractView {
    const BarrierTerms* barrier_terms{};
    std::optional<OptionType> option_type;
    double strike{};
    double payout{};
    bool asset_settlement{};
    kiyosi::SettlementTiming settlement_timing{};
};

BinaryBarrierContractView make_contract_view(const BinaryBarrierOption& option)
{
    const bool asset = option.payoff_type() == PayoffType::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {&option.barrier_terms(), option.option_type(), option.strike(),
            cash ? cash->payout() : option.barrier_level(), asset, SettlementTiming::at_expiry};
}

BinaryBarrierContractView make_contract_view(const TouchOption& option)
{
    const bool asset = option.payoff_type() == PayoffType::asset;
    const auto* cash = std::get_if<CashOrNothingPayoff>(&option.payoff());
    return {&option.barrier_terms(), std::nullopt, option.barrier_level(),
            cash ? cash->payout() : option.barrier_level(), asset, option.settlement_timing()};
}

Result<double> vanilla_digital(const BinaryBarrierContractView& option, const PricingContext& context, double time)
{
    if (!option.option_type)
        return checked_price(option.asset_settlement
                                 ? context.spot_price() * std::exp(-context.model_parameters().dividend_yield() * time)
                                 : option.payout * std::exp(-context.model_parameters().risk_free_rate() * time));
    const auto& terms = *option.barrier_terms;
    const AnalyticDigitalEngine engine;
    const auto price = option.asset_settlement
                           ? engine.price(*make_asset_or_nothing_option(*option.option_type, option.strike,
                                                                        terms.effective_date(), terms.expiry_date()),
                                          context)
                           : engine.price(*make_cash_or_nothing_option(*option.option_type, option.strike, option.payout,
                                                                       terms.effective_date(), terms.expiry_date()),
                                          context);
    return price;
}

double terminal_payoff(const BinaryBarrierContractView& option, double spot, bool touched)
{
    const auto& terms = *option.barrier_terms;
    const bool in_money = !option.option_type || (*option.option_type == OptionType::call ? spot > option.strike : spot < option.strike);
    return terms.is_knock_in() == touched && in_money ? (option.asset_settlement ? spot : option.payout) : 0.0;
}

Result<double> price_contract(const BinaryBarrierContractView& option, const PricingContext& context)
{
    const auto& terms = *option.barrier_terms;
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), terms.effective_date(), terms.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (terms.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(terms.observation_dates(), terms.effective_date(), terms.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const auto prior_touch = terms.was_touched_before(context.valuation_time());
    if (!prior_touch) return std::unexpected(prior_touch.error());
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), terms.expiry_date());
    const double spot = context.spot_price();
    const bool upper = terms.is_up();
    const bool knock_in = terms.is_knock_in();
    const bool observed_now = terms.is_monitored_at(context.valuation_time());
    const bool touched_now = observed_now && terms.is_breached_by(spot);
    const bool touched = *prior_touch || touched_now;
    if (*prior_touch && option.settlement_timing == SettlementTiming::at_hit)
        return 0.0;
    if (time == 0.0)
        return checked_price(terminal_payoff(option, spot, touched));
    if (touched) {
        if (!knock_in) return 0.0;
        if (option.settlement_timing == SettlementTiming::at_hit)
            return checked_price(option.asset_settlement ? spot : option.payout);
        return vanilla_digital(option, context, time);
    }
    if (!terms.is_continuous() && start_of_day(terms.observation_dates().back()) <= context.valuation_time())
        return knock_in ? 0.0 : vanilla_digital(option, context, time);
    const double rate = context.model_parameters().risk_free_rate(), dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility(), volatility_time = volatility * std::sqrt(time);
    double barrier = terms.barrier_level();
    if (terms.observation_mode() == ObservationMode::scheduled)
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * volatility * std::sqrt(terms.mean_observation_year_fraction()));
    const auto monitoring_valid = validate_analytic_barrier_monitoring(terms, spot, barrier);
    if (!monitoring_valid) return std::unexpected(monitoring_valid.error());
    const double log_ratio = log_price_ratio(barrier, spot);
    if (option.settlement_timing == SettlementTiming::at_hit) {
        const double variance = volatility * volatility;
        return checked_price(option.payout * barrier_hit_discount(
                                                 std::abs(log_ratio), upper, rate - dividend - 0.5 * variance, variance, time, rate));
    }
    const double mu = (rate - dividend - .5 * volatility * volatility) / (volatility * volatility);
    const double log_moneyness = log_price_ratio(spot, option.strike);
    const double x1 = log_moneyness / volatility_time + (1 + mu) * volatility_time;
    const double x2 = -log_ratio / volatility_time + (1 + mu) * volatility_time;
    const double y1 = (2.0 * log_ratio + log_moneyness) / volatility_time + (1 + mu) * volatility_time;
    const double y2 = log_ratio / volatility_time + (1 + mu) * volatility_time;
    const bool down = !upper, call = option.option_type && *option.option_type == OptionType::call;
    const double phi = option.option_type ? (call ? 1.0 : -1.0)
                                          : (knock_in ? (down ? -1.0 : 1.0) : (down ? 1.0 : -1.0));
    const double eta = down ? 1.0 : -1.0;
    const double amount = option.asset_settlement ? spot : option.payout;
    const double log_discount = -(option.asset_settlement ? dividend : rate) * time;
    const double reflected_log_discount = log_discount + (2 * (option.asset_settlement ? mu + 1 : mu)) * log_ratio;
    const double shift = option.asset_settlement ? 0.0 : volatility_time;
    const double f1 = scaled_normal_cdf(amount, log_discount, phi * x1 - phi * shift);
    const double f2 = scaled_normal_cdf(amount, log_discount, phi * x2 - phi * shift);
    const double f3 = scaled_normal_cdf(amount, reflected_log_discount, eta * y1 - eta * shift);
    const double f4 = scaled_normal_cdf(amount, reflected_log_discount, eta * y2 - eta * shift);
    double value = 0.0;
    if (!option.option_type) {
        value = knock_in ? f2 + f4 : f2 - f4;
    } else if (knock_in) {
        if (call) value = down ? (option.strike > barrier ? f3 : f1 - f2 + f4)
                               : (option.strike > barrier ? f1 : f2 - f3 + f4);
        else value = down ? (option.strike > barrier ? f2 - f3 + f4 : f1)
                          : (option.strike > barrier ? f1 - f2 + f4 : f3);
    } else {
        if (call) value = down ? (option.strike > barrier ? f1 - f3 : f2 - f4)
                               : (option.strike > barrier ? 0.0 : f1 - f2 + f3 - f4);
        else value = down ? (option.strike > barrier ? f1 - f2 + f3 - f4 : 0.0)
                          : (option.strike > barrier ? f2 - f4 : f1 - f3);
    }
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "binary barrier pricing produced a non-finite result"});
    return std::max(value, 0.0);
}
} // namespace

Result<double> AnalyticBinaryBarrierEngine::price(
    const BinaryBarrierOption& option, const PricingContext& context) const
{
    return price_contract(make_contract_view(option), context);
}

Result<double> AnalyticBinaryBarrierEngine::price(
    const TouchOption& option, const PricingContext& context) const
{
    return price_contract(make_contract_view(option), context);
}
Result<PricingResult> AnalyticBinaryBarrierEngine::price_with_greeks(const BinaryBarrierOption& option, const PricingContext& context,
                                                                     GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

Result<PricingResult> AnalyticBinaryBarrierEngine::price_with_greeks(const TouchOption& option, const PricingContext& context,
                                                                     GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

} // namespace kiyosi
