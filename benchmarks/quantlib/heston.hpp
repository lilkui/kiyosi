#pragma once
#include "common.hpp"

namespace bench {
inline QuantLib::Size heston_count(Request& request, const std::string& key, bool zero = false) {
    const double value = request.number(key);
    if (value < (zero ? 0 : 1) || value > 10000000 || std::floor(value) != value)
        throw std::runtime_error("invalid integer: " + key);
    return static_cast<QuantLib::Size>(value);
}
inline Price heston(Request& request, const std::string& family) {
    using namespace QuantLib;
    const auto type = request.option();
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double spot = request.number("spot"), strike = request.number("strike");
    const double rate = request.number("rate"), dividend = request.number("dividend");
    const double v0 = request.number("initial_variance"), theta = request.number("long_run_variance");
    const double kappa = request.number("mean_reversion_speed"), xi = request.number("vol_of_vol"), rho = request.number("correlation");
    if (spot <= 0 || strike <= 0 || v0 <= 0 || theta <= 0 || kappa <= 0 || xi <= 0 || std::abs(rho) >= 1)
        throw std::runtime_error("benchmark requires nondegenerate Heston inputs");
    Settings::instance().evaluationDate() = valuation;
    const DayCounter dc = Actual365Fixed();
    auto model = ext::make_shared<HestonModel>(ext::make_shared<HestonProcess>(
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, rate, dc)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, dividend, dc)),
        Handle<Quote>(ext::make_shared<SimpleQuote>(spot)), v0, kappa, theta, xi, rho));
    ext::shared_ptr<Exercise> exercise;
    if (family == "heston_fd_american") exercise = ext::make_shared<AmericanExercise>(valuation, expiry);
    else exercise = ext::make_shared<EuropeanExercise>(expiry);
    auto option = ext::make_shared<VanillaOption>(ext::make_shared<PlainVanillaPayoff>(type, strike), exercise);
    if (family == "heston_analytic") {
        const double tolerance = request.number("relative_tolerance");
        const Size evaluations = heston_count(request, "max_evaluations");
        if (tolerance <= 0) throw std::runtime_error("relative_tolerance must be positive");
        option->setPricingEngine(ext::make_shared<AnalyticHestonEngine>(model, tolerance, evaluations));
    } else {
        const Size t = heston_count(request, "time_steps"), x = heston_count(request, "asset_steps"), v = heston_count(request, "variance_steps");
        const Size damping = heston_count(request, "damping_steps", true);
        if (request.take("scheme") != "hundsdorfer") throw std::runtime_error("unsupported Heston scheme");
        option->setPricingEngine(ext::make_shared<FdHestonVanillaEngine>(model, t, x, v, damping, FdmSchemeDesc::Hundsdorfer()));
    }
    return [option]() { option->recalculate(); return option->NPV(); };
}
}
