#pragma once
#include "fd.hpp"

namespace bench {
inline Price local_vol(Request& request) {
    using namespace QuantLib;
    const auto valuation = request.date("valuation"), expiry = request.date("expiry");
    if (expiry <= valuation) throw std::runtime_error("benchmark expiry must follow valuation");
    const double low = request.number("surface_low"), high = request.number("surface_high");
    if (low <= 0 || high <= low) throw std::runtime_error("ordered positive surface spots required");
    auto matrix = ext::make_shared<Matrix>(2, 2);
    (*matrix)[0][0] = request.number("surface_start_low");
    (*matrix)[1][0] = request.number("surface_start_high");
    (*matrix)[0][1] = request.number("surface_end_low");
    (*matrix)[1][1] = request.number("surface_end_high");
    for (const double value : *matrix) if (value <= 0) throw std::runtime_error("positive local volatility required");
    const DayCounter dc = Actual365Fixed();
    auto surface = ext::make_shared<FixedLocalVolSurface>(valuation,
        std::vector<Time>{0., dc.yearFraction(valuation, expiry)}, std::vector<Real>{low, high}, matrix, dc);
    // Spot extrapolation uses the surface's explicit constant endpoint policy.
    surface->enableExtrapolation();
    const auto operation = request.take("operation");
    if (operation == "surface_point") {
        const auto date = request.date("query_date");
        const double spot = request.number("query_spot");
        if (date < valuation || date > expiry || spot <= 0) throw std::runtime_error("surface query requires covered time and positive spot");
        const double time = dc.yearFraction(valuation, date);
        return [surface, time, spot]() { return surface->localVol(time, spot); };
    }
    if (operation != "price") throw std::runtime_error("invalid local volatility operation");
    const double spot = request.number("spot"), rate = request.number("rate"), dividend = request.number("dividend"), mesher_vol = request.number("mesher_vol");
    if (spot <= 0 || mesher_vol <= 0) throw std::runtime_error("positive spot and mesher volatility required");
    Settings::instance().evaluationDate() = valuation;
    auto process = ext::make_shared<GeneralizedBlackScholesProcess>(
        Handle<Quote>(ext::make_shared<SimpleQuote>(spot)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, dividend, dc)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, rate, dc)),
        Handle<BlackVolTermStructure>(ext::make_shared<BlackConstantVol>(valuation, NullCalendar(), mesher_vol, dc)),
        Handle<LocalVolTermStructure>(surface));
    return fd_product(request, process, valuation, expiry, spot, true);
}
}
