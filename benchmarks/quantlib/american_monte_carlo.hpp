#pragma once
#include "monte_carlo.hpp"

namespace bench {
inline Price american_monte_carlo(Request& request) {
    using namespace QuantLib;
    const auto side = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend"), volatility = request.number("volatility");
    if (expiry <= valuation || spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("invalid American MC contract/market");
    const auto seed = mc_integer(request, "seed", 4294967295ULL);
    const auto calibrationSeed = mc_integer(request, "calibration_seed", 4294967295ULL);
    const auto paths = mc_integer(request, "path_count", 10000000);
    const auto calibrationPaths = mc_integer(request, "calibration_path_count", 10000000);
    const auto steps = mc_integer(request, "step_count", 10000);
    if (steps < 2 || seed == calibrationSeed || request.number("workers") != 1
        || request.take("sampling") != "pseudo_random" || request.take("antithetic") != "false"
        || request.take("control_variate") != "false" || request.take("basis") != "quadratic_monomial")
        throw std::runtime_error("American MC requires one worker, independent nonzero seeds, pseudorandom, quadratic monomial, no antithetic/control variate and >=2 steps");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    auto option = ext::make_shared<VanillaOption>(ext::make_shared<PlainVanillaPayoff>(side, strike), ext::make_shared<AmericanExercise>(valuation, expiry));
    option->setPricingEngine(MakeMCAmericanEngine<PseudoRandom>(process)
        .withSteps(steps).withSamples(paths).withSeed(seed)
        .withAntitheticVariate(false).withControlVariate(false)
        .withPolynomialOrder(2).withBasisSystem(LsmBasisSystem::Monomial)
        .withCalibrationSamples(calibrationPaths).withAntitheticVariateCalibration(false)
        .withSeedCalibration(calibrationSeed));
    // MCLongstaffSchwartzEngine::calculate rebuilds and calibrates its path pricer each call.
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
