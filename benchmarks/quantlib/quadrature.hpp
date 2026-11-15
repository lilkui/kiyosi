#pragma once
#include "common.hpp"

namespace bench {
inline Price quadrature(Request& request, bool digital) {
    using namespace QuantLib;
    const auto side = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double volatility = request.number("volatility");
    if (spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("positive spot, strike and volatility required");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    ext::shared_ptr<StrikedTypePayoff> payoff;
    if (digital) {
        const auto kind = request.take("payoff");
        if (kind == "cash") payoff = ext::make_shared<CashOrNothingPayoff>(side, strike, request.number("cash"));
        else if (kind == "asset") payoff = ext::make_shared<AssetOrNothingPayoff>(side, strike);
        else throw std::runtime_error("invalid digital payoff: " + kind);
    } else payoff = ext::make_shared<PlainVanillaPayoff>(side, strike);
    auto option = ext::make_shared<VanillaOption>(payoff, ext::make_shared<EuropeanExercise>(expiry));
    option->setPricingEngine(ext::make_shared<IntegralEngine>(process));
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
