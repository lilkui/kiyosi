#pragma once
#include "common.hpp"

namespace bench {
inline QuantLib::Size fd_count(Request& request, const std::string& key) {
    const double value = request.number(key);
    if (value < 2 || value > 1000000 || value != std::floor(value)) throw std::runtime_error("invalid FD count: " + key);
    return static_cast<QuantLib::Size>(value);
}
inline Price fd_product(Request& request, const QuantLib::ext::shared_ptr<QuantLib::GeneralizedBlackScholesProcess>& process,
                        QuantLib::Date valuation, QuantLib::Date expiry, double spot, bool local_vol = false) {
    using namespace QuantLib;
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const Size space = fd_count(request, "space"), time = fd_count(request, "time");
    const double damping_value = request.number("damping");
    if (damping_value < 0 || damping_value > time || damping_value != std::floor(damping_value)) throw std::runtime_error("invalid damping");
    const Size damping = static_cast<Size>(damping_value);
    const auto scheme_name = request.take("scheme");
    if (scheme_name != "crank_nicolson" && scheme_name != "implicit_euler") throw std::runtime_error("invalid FD scheme: " + scheme_name);
    const auto scheme = scheme_name == "implicit_euler" ? FdmSchemeDesc::ImplicitEuler() : FdmSchemeDesc::CrankNicolson();
    if (request.take("domain") != "automatic") throw std::runtime_error("QuantLib FD domain must be automatic");
    const auto product = request.take("product");
    ext::shared_ptr<StrikedTypePayoff> payoff;
    ext::shared_ptr<Exercise> exercise = ext::make_shared<EuropeanExercise>(expiry);
    auto payout = product == "vanilla" || product == "barrier" ? "vanilla" : request.take("payout");
    double cash = payout == "cash" ? request.number("cash") : 0;
    if (payout == "cash" && cash <= 0) throw std::runtime_error("cash must be positive");
    if (payout != "cash" && payout != "asset" && payout != "vanilla") throw std::runtime_error("invalid payout");
    const auto make_payoff = [&](Option::Type type, double strike) -> ext::shared_ptr<StrikedTypePayoff> {
        if (strike <= 0) throw std::runtime_error("strike must be positive");
        if (payout == "cash") return ext::make_shared<CashOrNothingPayoff>(type, strike, cash);
        if (payout == "asset") return ext::make_shared<AssetOrNothingPayoff>(type, strike);
        return ext::make_shared<PlainVanillaPayoff>(type, strike);
    };
    if (product == "vanilla" || product == "digital") {
        const auto type = request.option();
        payoff = make_payoff(type, request.number("strike"));
        if (product == "vanilla") {
            const auto style = request.take("exercise");
            if (style == "american") exercise = ext::make_shared<AmericanExercise>(valuation, expiry);
            else if (style != "european") throw std::runtime_error("invalid exercise");
        }
        auto option = ext::make_shared<VanillaOption>(payoff, exercise);
        option->setPricingEngine(ext::make_shared<FdBlackScholesVanillaEngine>(process, time, space, damping, scheme, local_vol));
        return [option]() { option->recalculate(); return option->NPV(); };
    }
    const double barrier = request.number("barrier");
    const auto name = request.take("barrier_type");
    Barrier::Type type;
    if (name == "down_out") type = Barrier::DownOut;
    else if (name == "down_in") type = Barrier::DownIn;
    else if (name == "up_out") type = Barrier::UpOut;
    else if (name == "up_in") type = Barrier::UpIn;
    else throw std::runtime_error("invalid barrier_type");
    const bool up = type == Barrier::UpIn || type == Barrier::UpOut;
    const bool knock_in = type == Barrier::UpIn || type == Barrier::DownIn;
    if (barrier <= 0 || (up ? spot >= barrier : spot <= barrier)) throw std::runtime_error("positive initially untouched barrier required");
    double rebate = 0;
    if (product == "touch") {
        const auto settlement = request.take("settlement");
        if (knock_in ? settlement != "at_hit" : settlement != "at_expiry") throw std::runtime_error("FD subset requires one-touch at_hit or no-touch at_expiry");
        const auto payoff_type = (up == knock_in) ? Option::Call : Option::Put;
        payoff = make_payoff(payoff_type, barrier);
        if (knock_in) rebate = payout == "cash" ? cash : barrier;
        type = up ? Barrier::UpOut : Barrier::DownOut;
    } else if (product == "barrier" || product == "binary_barrier") {
        const auto payoff_type = request.option();
        payoff = make_payoff(payoff_type, request.number("strike"));
    } else throw std::runtime_error("invalid FD product: " + product);
    auto option = ext::make_shared<BarrierOption>(type, barrier, rebate, payoff, exercise);
    option->setPricingEngine(ext::make_shared<FdBlackScholesBarrierEngine>(process, time, space, damping, scheme, local_vol));
    return [option]() { option->recalculate(); return option->NPV(); };
}
inline Price fd(Request& request) {
    using namespace QuantLib;
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    const double spot = request.number("spot"), rate = request.number("rate"), dividend = request.number("dividend"), volatility = request.number("volatility");
    if (spot <= 0 || volatility <= 0) throw std::runtime_error("positive spot and volatility required");
    auto process = flat_bsm_process(valuation, spot, rate, dividend, volatility);
    return fd_product(request, process, valuation, expiry, spot);
}
}
