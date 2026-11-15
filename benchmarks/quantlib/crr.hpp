#pragma once
#include "common.hpp"

namespace bench {
inline Price crr(Request& request, bool barrier) {
    using namespace QuantLib;
    const auto side = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double volatility = request.number("volatility"), steps = request.number("step_count");
    if (spot <= 0 || strike <= 0 || volatility <= 0) throw std::runtime_error("positive spot, strike and volatility required");
    if (steps < 1 || steps > 1000000 || steps != std::floor(steps))
        throw std::runtime_error("step_count must be an integer in 1..=1000000");
    const auto count = static_cast<Size>(steps);
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    const auto payoff = ext::make_shared<PlainVanillaPayoff>(side, strike);
    ext::shared_ptr<Instrument> option;
    if (barrier) {
        const double level = request.number("barrier");
        const auto kind = request.take("barrier_type");
        Barrier::Type type;
        if (kind == "down_out") type = Barrier::DownOut;
        else if (kind == "down_in") type = Barrier::DownIn;
        else if (kind == "up_out") type = Barrier::UpOut;
        else if (kind == "up_in") type = Barrier::UpIn;
        else throw std::runtime_error("invalid barrier_type: " + kind);
        if (level <= 0 || ((type == Barrier::DownOut || type == Barrier::DownIn) ? spot <= level : spot >= level))
            throw std::runtime_error("benchmark requires positive untouched barrier");
        option = ext::make_shared<BarrierOption>(type, level, 0.0, payoff, ext::make_shared<EuropeanExercise>(expiry));
        // Equal counts disable Boyle-Lau; this explicit template excludes Derman-Kani.
        option->setPricingEngine(ext::make_shared<BinomialBarrierEngine<CoxRossRubinstein, DiscretizedBarrierOption>>(process, count, count));
    } else {
        const auto style = request.take("exercise");
        ext::shared_ptr<Exercise> exercise;
        if (style == "european") exercise = ext::make_shared<EuropeanExercise>(expiry);
        else if (style == "american") exercise = ext::make_shared<AmericanExercise>(valuation, expiry);
        else throw std::runtime_error("invalid exercise: " + style);
        option = ext::make_shared<VanillaOption>(payoff, exercise);
        option->setPricingEngine(ext::make_shared<BinomialVanillaEngine<CoxRossRubinstein>>(process, count));
    }
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
