// Offline refinement probe; never linked into the timed executable.
#include "heston.hpp"
#include <iomanip>
#include <iostream>

int main(int argc, char** argv) {
    using namespace QuantLib;
    try {
        if (std::string(QL_VERSION) != "1.44") throw std::runtime_error("requires QuantLib 1.44");
        bench::Request request(argc, argv);
        const auto method = request.take("method");
        const Date start(1, January, 2025), end(1, January, 2026);
        Settings::instance().evaluationDate() = start;
        const Actual365Fixed dc;
        auto model = ext::make_shared<HestonModel>(ext::make_shared<HestonProcess>(
            Handle<YieldTermStructure>(ext::make_shared<FlatForward>(start, .05, dc)),
            Handle<YieldTermStructure>(ext::make_shared<FlatForward>(start, .02, dc)),
            Handle<Quote>(ext::make_shared<SimpleQuote>(100)), .04, 1.5, .04, .3, -.7));
        if (method == "analytic") {
            const double tolerance = request.number("tolerance");
            const auto quadrature = request.take("quadrature");
            ext::shared_ptr<AnalyticHestonEngine> engine;
            if (quadrature == "lobatto") engine = ext::make_shared<AnalyticHestonEngine>(model, tolerance, 100000);
            else if (quadrature == "exp_sinh") engine = ext::make_shared<AnalyticHestonEngine>(model, AnalyticHestonEngine::Gatheral, AnalyticHestonEngine::Integration::expSinh(tolerance));
            else throw std::runtime_error("unknown quadrature");
            VanillaOption option(ext::make_shared<PlainVanillaPayoff>(Option::Call, 100), ext::make_shared<EuropeanExercise>(end));
            request.finish();
            option.setPricingEngine(engine);
            std::cout << std::setprecision(17) << "{\"price\":" << option.NPV() << ",\"evaluations\":" << engine->numberOfEvaluations() << ",\"quantlib_version\":\"" << QL_VERSION << "\"}\n";
        } else {
            const Size t = bench::heston_count(request, "time_steps"), x = bench::heston_count(request, "asset_steps"), v = bench::heston_count(request, "variance_steps");
            const auto exercise = request.take("exercise");
            ext::shared_ptr<Exercise> schedule;
            if (exercise == "american") schedule = ext::make_shared<AmericanExercise>(start, end);
            else schedule = ext::make_shared<EuropeanExercise>(end);
            auto engine = ext::make_shared<FdHestonVanillaEngine>(model, t, x, v, 0, FdmSchemeDesc::Hundsdorfer());
            VanillaOption option(ext::make_shared<PlainVanillaPayoff>(exercise == "american" ? Option::Put : Option::Call, 100), schedule);
            request.finish();
            option.setPricingEngine(engine);
            const double value = option.NPV();
            auto desc = engine->getSolverDesc(1.0);
            auto a = desc.mesher->locations(0), b = desc.mesher->locations(1);
            auto [amin, amax] = std::minmax_element(a.begin(), a.end());
            auto [bmin, bmax] = std::minmax_element(b.begin(), b.end());
            std::cout << std::setprecision(17) << "{\"price\":" << value << ",\"asset_domain\":[" << std::exp(*amin) << ',' << std::exp(*amax) << "],\"variance_domain\":[" << *bmin << ',' << *bmax << "],\"quantlib_version\":\"" << QL_VERSION << "\"}\n";
        }
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
