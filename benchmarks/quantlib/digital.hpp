#pragma once
#include "common.hpp"

namespace bench {
inline Price digital(Request& request) {
    using namespace QuantLib;
    const auto type = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double volatility = request.number("volatility");
    if (spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("positive spot, strike and volatility required");
    ext::shared_ptr<StrikedTypePayoff> payoff;
    const auto payout = request.take("payout");
    if (payout == "cash") {
        const double cash = request.number("cash");
        if (cash <= 0) throw std::runtime_error("positive cash required");
        payoff = ext::make_shared<CashOrNothingPayoff>(type, strike, cash);
    } else if (payout == "asset") payoff = ext::make_shared<AssetOrNothingPayoff>(type, strike);
    else throw std::runtime_error("invalid payout: " + payout);
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    auto option = ext::make_shared<VanillaOption>(payoff, ext::make_shared<EuropeanExercise>(expiry));
    option->setPricingEngine(ext::make_shared<AnalyticEuropeanEngine>(process));
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
