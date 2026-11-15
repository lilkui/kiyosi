// Offline reference checks; no reference work runs in measured price batches.
#include <ql/quantlib.hpp>
#include <iostream>
#include <iomanip>

int main() {
    using namespace QuantLib;
    const Date at(1, January, 2025), end(1, January, 2026);
    Settings::instance().evaluationDate() = at;
    const DayCounter dc = Actual365Fixed();
    auto process = ext::make_shared<BlackScholesMertonProcess>(
        Handle<Quote>(ext::make_shared<SimpleQuote>(100)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(at, 0.02, dc)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(at, 0.05, dc)),
        Handle<BlackVolTermStructure>(ext::make_shared<BlackConstantVol>(at, NullCalendar(), 0.2, dc)));
    VanillaOption option(ext::make_shared<PlainVanillaPayoff>(Option::Put, 100), ext::make_shared<AmericanExercise>(at, end));
    for (const auto [space,time] : std::vector<std::pair<Size,Size>>{
        {800,6400},{1600,6400},{3200,6400},{3200,12800},{3200,25600}}) {
        option.setPricingEngine(ext::make_shared<FdBlackScholesVanillaEngine>(process, time, space, 2, FdmSchemeDesc::CrankNicolson()));
        option.recalculate();
        std::cout << std::setprecision(17) << "{\"space\":" << space << ",\"time\":" << time << ",\"price\":" << option.NPV() << "}\n";
    }
}
