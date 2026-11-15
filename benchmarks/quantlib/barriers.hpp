#pragma once
#include "common.hpp"

namespace bench {
inline Price barriers(Request& request, const std::string& family) {
    using namespace QuantLib;
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), rate = request.number("rate");
    const double dividend = request.number("dividend"), volatility = request.number("volatility");
    if (spot <= 0 || volatility <= 0) throw std::runtime_error("positive spot and volatility required");
    const auto activation = request.take("activation");
    if (activation != "in" && activation != "out") throw std::runtime_error("activation must be in or out");
    const bool knockIn = activation == "in";
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    ext::shared_ptr<Instrument> instrument;
    if (family == "double_barrier_bsm" || family == "double_touch_bsm") {
        const double lower = request.number("lower"), upper = request.number("upper");
        if (!(0 < lower && lower < spot && spot < upper)) throw std::runtime_error("benchmark requires initially untouched barriers");
        ext::shared_ptr<StrikedTypePayoff> payoff;
        ext::shared_ptr<PricingEngine> engine;
        if (family == "double_barrier_bsm") {
            const auto side = request.option();
            const double strike = request.number("strike");
            if (!(lower < strike && strike < upper)) throw std::runtime_error("strike must be between barriers");
            payoff = ext::make_shared<PlainVanillaPayoff>(side, strike);
            const double series = request.number("series");
            if (series < 1 || series > 1000 || series != std::floor(series)) throw std::runtime_error("invalid series");
            engine = ext::make_shared<AnalyticDoubleBarrierEngine>(process, static_cast<int>(series));
        } else {
            const double payout = request.number("payout");
            if (payout <= 0) throw std::runtime_error("positive payout required");
            // The native double binary engine pays cash unconditionally on activation/survival.
            payoff = ext::make_shared<CashOrNothingPayoff>(Option::Call, spot, payout);
            engine = ext::make_shared<AnalyticDoubleBarrierBinaryEngine>(process);
        }
        auto option = ext::make_shared<DoubleBarrierOption>(knockIn ? DoubleBarrier::KnockIn : DoubleBarrier::KnockOut, lower, upper, 0, payoff, ext::make_shared<EuropeanExercise>(expiry));
        option->setPricingEngine(engine);
        instrument = option;
    } else {
        const double barrier = request.number("barrier");
        const auto direction = request.take("direction");
        if (direction != "up" && direction != "down") throw std::runtime_error("direction must be up or down");
        const bool up = direction == "up";
        if (!(barrier > 0 && (up ? spot < barrier : spot > barrier))) throw std::runtime_error("benchmark requires initially untouched barriers");
        const auto type = up ? (knockIn ? Barrier::UpIn : Barrier::UpOut) : (knockIn ? Barrier::DownIn : Barrier::DownOut);
        ext::shared_ptr<StrikedTypePayoff> payoff;
        ext::shared_ptr<Exercise> exercise;
        ext::shared_ptr<PricingEngine> engine;
        double rebate = 0;
        if (family == "single_barrier_bsm") {
            const auto side = request.option();
            const double strike = request.number("strike");
            if (strike <= 0) throw std::runtime_error("positive strike required");
            payoff = ext::make_shared<PlainVanillaPayoff>(side, strike);
            rebate = request.number("rebate");
            if (rebate < 0) throw std::runtime_error("nonnegative rebate required");
            exercise = ext::make_shared<EuropeanExercise>(expiry);
            engine = ext::make_shared<AnalyticBarrierEngine>(process);
        } else {
            const auto kind = request.take("payoff");
            if (kind != "cash" && kind != "asset") throw std::runtime_error("payoff must be cash or asset");
            const double payout = kind == "cash" ? request.number("payout") : 0;
            if (kind == "cash" && payout <= 0) throw std::runtime_error("positive payout required");
            const auto side = family == "touch_bsm" ? (up ? Option::Call : Option::Put) : request.option();
            const double strike = family == "touch_bsm" ? barrier : request.number("strike");
            if (strike <= 0) throw std::runtime_error("positive strike required");
            bool atExpiry = true;
            if (family == "touch_bsm") {
                const auto settlement = request.take("settlement");
                if (settlement != "expiry" && !(settlement == "hit" && knockIn)) throw std::runtime_error("unmatched settlement");
                atExpiry = settlement == "expiry";
            }
            if (kind == "cash") payoff = ext::make_shared<CashOrNothingPayoff>(side, strike, payout);
            // On continuous first passage S_tau=H, so asset-at-hit is exactly cash H.
            else if (!atExpiry) payoff = ext::make_shared<CashOrNothingPayoff>(side, barrier, barrier);
            else payoff = ext::make_shared<AssetOrNothingPayoff>(side, strike);
            exercise = ext::make_shared<AmericanExercise>(valuation, expiry, atExpiry);
            if (family == "binary_barrier_bsm") engine = ext::make_shared<AnalyticBinaryBarrierEngine>(process);
            else if (knockIn) engine = ext::make_shared<AnalyticDigitalAmericanEngine>(process);
            else engine = ext::make_shared<AnalyticDigitalAmericanKOEngine>(process);
        }
        if (family == "touch_bsm") {
            auto option = ext::make_shared<VanillaOption>(payoff, exercise);
            option->setPricingEngine(engine);
            instrument = option;
        } else {
            auto option = ext::make_shared<BarrierOption>(type, barrier, rebate, payoff, exercise);
            option->setPricingEngine(engine);
            instrument = option;
        }
    }
    return [instrument]() { instrument->recalculate(); return instrument->NPV(); };
}
}
