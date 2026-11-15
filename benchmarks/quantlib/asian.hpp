#pragma once
#include "common.hpp"

namespace bench {
inline Price asian(Request& request, bool arithmetic) {
    using namespace QuantLib;
    const auto type = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    const auto averagingStart = request.date("averaging_start");
    if (expiry <= valuation || averagingStart != valuation)
        throw std::runtime_error("Asian benchmark requires future expiry and averaging_start equal to valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double volatility = request.number("volatility");
    if (spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("positive spot, strike and volatility required");
    const double carry = rate - dividend;
    if (arithmetic && (std::abs(carry) < 1e-6 || std::abs(carry + volatility * volatility) < 1e-6
        || std::abs(2 * carry + volatility * volatility) < 1e-6))
        throw std::runtime_error("arithmetic Asian benchmark requires ordinary carry and nonsingular Levy denominators");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    const auto payoff = ext::make_shared<PlainVanillaPayoff>(type, strike);
    const auto exercise = ext::make_shared<EuropeanExercise>(expiry);
    // A supplied start date selects QuantLib's seasoned geometric path, which it rejects.
    auto option = arithmetic
        ? ext::make_shared<ContinuousAveragingAsianOption>(Average::Arithmetic, averagingStart, payoff, exercise)
        : ext::make_shared<ContinuousAveragingAsianOption>(Average::Geometric, payoff, exercise);
    if (arithmetic) option->setPricingEngine(ext::make_shared<ContinuousArithmeticAsianLevyEngine>(
        process, Handle<Quote>(ext::make_shared<SimpleQuote>(0.0))));
    else option->setPricingEngine(ext::make_shared<AnalyticContinuousGeometricAveragePriceAsianEngine>(process));
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
