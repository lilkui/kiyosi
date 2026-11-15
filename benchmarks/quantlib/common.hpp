#pragma once
#include <ql/quantlib.hpp>
#include <cmath>
#include <functional>
#include <map>
#include <stdexcept>
#include <string>

namespace bench {
using Price = std::function<double()>;
struct Request {
    std::map<std::string, std::string> values;
    Request(int argc, char** argv) {
        for (int i = 1; i < argc; ++i) {
            const std::string arg(argv[i]);
            const auto pos = arg.find('=');
            if (pos == std::string::npos || !values.emplace(arg.substr(0, pos), arg.substr(pos + 1)).second)
                throw std::runtime_error("expected unique key=value parameters");
        }
    }
    std::string take(const std::string& key) {
        const auto item = values.find(key);
        if (item == values.end()) throw std::runtime_error("missing parameter: " + key);
        const auto value = item->second;
        values.erase(item);
        return value;
    }
    double number(const std::string& key) {
        const auto value = take(key);
        std::size_t end;
        const double result = std::stod(value, &end);
        if (end != value.size() || !std::isfinite(result)) throw std::runtime_error("invalid number: " + key);
        return result;
    }
    QuantLib::Date date(const std::string& key) {
        const auto value = take(key);
        if (value.size() != 10 || value[4] != '-' || value[7] != '-') throw std::runtime_error("invalid ISO date");
        return QuantLib::DateParser::parseISO(value);
    }
    QuantLib::Option::Type option() {
        const auto value = take("option");
        if (value == "call") return QuantLib::Option::Call;
        if (value == "put") return QuantLib::Option::Put;
        throw std::runtime_error("invalid option: " + value);
    }
    void finish() const {
        if (!values.empty()) throw std::runtime_error("unknown parameter: " + values.begin()->first);
    }
};
inline QuantLib::ext::shared_ptr<QuantLib::BlackScholesMertonProcess> flat_bsm_process(
    QuantLib::Date valuation, double spot, double rate, double dividend, double volatility) {
    using namespace QuantLib;
    Settings::instance().evaluationDate() = valuation;
    const DayCounter dc = Actual365Fixed();
    return ext::make_shared<BlackScholesMertonProcess>(
        Handle<Quote>(ext::make_shared<SimpleQuote>(spot)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, dividend, dc)),
        Handle<YieldTermStructure>(ext::make_shared<FlatForward>(valuation, rate, dc)),
        Handle<BlackVolTermStructure>(ext::make_shared<BlackConstantVol>(valuation, NullCalendar(), volatility, dc)));
}
}
