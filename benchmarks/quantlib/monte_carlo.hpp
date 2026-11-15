#pragma once
#include "common.hpp"

namespace bench {
inline QuantLib::Size mc_integer(Request& request, const std::string& key, QuantLib::Size upper) {
    const auto value = request.number(key);
    if (value < 1 || value > upper || value != std::floor(value)) throw std::runtime_error("invalid positive integer: " + key);
    return static_cast<QuantLib::Size>(value);
}
inline Price monte_carlo(Request& request, bool barrier) {
    using namespace QuantLib;
    const auto side = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend"), volatility = request.number("volatility");
    if (expiry <= valuation || spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("invalid MC contract/market");
    const auto seed = mc_integer(request, "seed", 4294967295ULL);
    if (barrier && seed == 5) throw std::runtime_error("barrier seed 5 couples QuantLib Gaussian and auxiliary crossing streams");
    const auto paths = mc_integer(request, "path_count", 10000000);
    if (request.number("workers") != 1 || request.take("sampling") != "pseudo_random" || request.take("antithetic") != "false" || request.take("control_variate") != "false")
        throw std::runtime_error("MC benchmark requires one worker, pseudorandom, no antithetic/control variate");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    auto payoff = ext::make_shared<PlainVanillaPayoff>(side, strike);
    auto exercise = ext::make_shared<EuropeanExercise>(expiry);
    ext::shared_ptr<Instrument> instrument;
    if (barrier) {
        const double level = request.number("barrier"), rebate = request.number("rebate");
        const auto direction = request.take("direction"), activation = request.take("activation");
        if ((direction != "up" && direction != "down") || (activation != "in" && activation != "out")) throw std::runtime_error("invalid barrier direction/activation");
        const bool up = direction == "up", in = activation == "in";
        if (!(level > 0 && (up ? spot < level : spot > level)) || rebate != 0) throw std::runtime_error("MC benchmark requires an untouched zero-rebate barrier");
        const auto steps = mc_integer(request, "step_count", 10000);
        const auto type = up ? (in ? Barrier::UpIn : Barrier::UpOut) : (in ? Barrier::DownIn : Barrier::DownOut);
        auto option = ext::make_shared<BarrierOption>(type, level, rebate, payoff, exercise);
        option->setPricingEngine(MakeMCBarrierEngine<PseudoRandom>(process).withSteps(steps).withSamples(paths).withSeed(seed).withBias(false).withAntitheticVariate(false).withBrownianBridge(false));
        instrument = option;
    } else {
        auto option = ext::make_shared<VanillaOption>(payoff, exercise);
        // One exact terminal step; no otherwise unused Kiyosi path-grid work.
        option->setPricingEngine(MakeMCEuropeanEngine<PseudoRandom>(process).withSteps(1).withSamples(paths).withSeed(seed).withAntitheticVariate(false).withBrownianBridge(false));
        instrument = option;
    }
    return [instrument]() { instrument->recalculate(); return instrument->NPV(); };
}
}
