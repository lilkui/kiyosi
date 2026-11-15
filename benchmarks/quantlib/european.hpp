#pragma once
#include "common.hpp"

namespace bench {
inline Price european(Request& request) {
    using namespace QuantLib;
    const auto type = request.option();
    const auto valuation = request.date("valuation");
    const auto expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double volatility = request.number("volatility");
    if (spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("positive spot, strike and volatility required");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    auto option = ext::make_shared<VanillaOption>(ext::make_shared<PlainVanillaPayoff>(type, strike), ext::make_shared<EuropeanExercise>(expiry));
    option->setPricingEngine(ext::make_shared<AnalyticEuropeanEngine>(process));
    return [option]() {
        option->recalculate();
        return option->NPV();
    };
}
}
