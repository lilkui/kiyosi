#pragma once

#include <nanobind/nanobind.h>
#include <nanobind/stl/chrono.h>
#include <nanobind/stl/optional.h>
#include <nanobind/stl/string.h>
#include <nanobind/stl/vector.h>

#include <array>
#include <chrono>
#include <cstdint>
#include <exception>
#include <initializer_list>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace nb = nanobind;

namespace kiyosi::python_binding {

struct PythonDateAnnotation {};
struct PythonTimestampAnnotation {};
struct PythonValuationTimeAnnotation {};
struct PythonGreekRequestAnnotation {};
template <typename Enum>
struct EnumNames;

template <typename Enum>
struct PythonChoice {
    nb::handle source;
    operator Enum() const; // NOLINT(google-explicit-constructor): validate when passed to the core
};

template <>
struct EnumNames<OptionType> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, OptionType>>(
        {{"call", OptionType::call}, {"put", OptionType::put}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['call', 'put']");
};

template <>
struct EnumNames<BarrierType> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, BarrierType>>(
        {{"up_and_in", BarrierType::up_and_in}, {"up_and_out", BarrierType::up_and_out}, {"down_and_in", BarrierType::down_and_in}, {"down_and_out", BarrierType::down_and_out}});
    static constexpr auto Name = nb::detail::const_name(
        "typing.Literal['up_and_in', 'up_and_out', 'down_and_in', 'down_and_out']");
};

template <>
struct EnumNames<ObservationMode> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, ObservationMode>>(
        {{"continuous", ObservationMode::continuous}, {"scheduled", ObservationMode::scheduled}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['continuous', 'scheduled']");
};

template <>
struct EnumNames<BarrierTouchState> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, BarrierTouchState>>(
        {{"untouched", BarrierTouchState::untouched}, {"touched", BarrierTouchState::touched}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['untouched', 'touched']");
};

template <>
struct EnumNames<RebateTiming> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, RebateTiming>>(
        {{"at_hit", RebateTiming::at_hit}, {"at_expiry", RebateTiming::at_expiry}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['at_hit', 'at_expiry']");
};

template <>
struct EnumNames<SettlementTiming> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, SettlementTiming>>(
        {{"at_hit", SettlementTiming::at_hit}, {"at_expiry", SettlementTiming::at_expiry}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['at_hit', 'at_expiry']");
};

template <>
struct EnumNames<PayoffType> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, PayoffType>>(
        {{"cash", PayoffType::cash}, {"asset", PayoffType::asset}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['cash', 'asset']");
};

template <>
struct EnumNames<KnockInObservationMode> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, KnockInObservationMode>>(
        {{"every_trading_day", KnockInObservationMode::every_trading_day},
         {"at_expiry", KnockInObservationMode::at_expiry}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['every_trading_day', 'at_expiry']");
};

template <>
struct EnumNames<AutocallableBarrierState> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, AutocallableBarrierState>>(
        {{"none", AutocallableBarrierState::none}, {"knocked_out", AutocallableBarrierState::knocked_out}, {"knocked_in", AutocallableBarrierState::knocked_in}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['none', 'knocked_out', 'knocked_in']");
};

template <>
struct EnumNames<FiniteDifferenceScheme> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, FiniteDifferenceScheme>>(
        {{"explicit_euler", FiniteDifferenceScheme::explicit_euler},
         {"implicit_euler", FiniteDifferenceScheme::implicit_euler},
         {"crank_nicolson", FiniteDifferenceScheme::crank_nicolson}});
    static constexpr auto Name = nb::detail::const_name(
        "typing.Literal['explicit_euler', 'implicit_euler', 'crank_nicolson']");
};

template <>
struct EnumNames<MonteCarloBackend> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, MonteCarloBackend>>(
        {{"cpu", MonteCarloBackend::cpu}, {"cuda", MonteCarloBackend::cuda}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['cpu', 'cuda']");
};

template <>
struct EnumNames<CouponQuoteConvention> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, CouponQuoteConvention>>(
        {{"shift_maturity_coupon", CouponQuoteConvention::shift_maturity_coupon},
         {"preserve_maturity_coupon", CouponQuoteConvention::preserve_maturity_coupon}});
    static constexpr auto Name = nb::detail::const_name(
        "typing.Literal['shift_maturity_coupon', 'preserve_maturity_coupon']");
};

template <>
struct EnumNames<BusinessDayConvention> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, BusinessDayConvention>>(
        {{"following", BusinessDayConvention::following}, {"preceding", BusinessDayConvention::preceding}});
    static constexpr auto Name = nb::detail::const_name("typing.Literal['following', 'preceding']");
};

template <>
struct EnumNames<Greek> {
    static constexpr auto values = std::to_array<std::pair<std::string_view, Greek>>(
        {{"delta", Greek::delta}, {"gamma", Greek::gamma}, {"speed", Greek::speed}, {"theta", Greek::theta}, {"charm", Greek::charm}, {"color", Greek::color}, {"vega", Greek::vega}, {"vanna", Greek::vanna}, {"zomma", Greek::zomma}, {"rho", Greek::rho}});
    static constexpr auto Name = nb::detail::const_name(
        "typing.Literal['delta', 'gamma', 'speed', 'theta', 'charm', 'color', 'vega', 'vanna', 'zomma', 'rho']");
};

} // namespace kiyosi::python_binding

namespace nanobind::detail {

template <>
struct type_caster<kiyosi::python_binding::PythonDateAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonDateAnnotation,
                   const_name("datetime.date"))
};

template <>
struct type_caster<kiyosi::python_binding::PythonTimestampAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonTimestampAnnotation,
                   const_name("datetime.datetime"))
};

template <>
struct type_caster<kiyosi::python_binding::PythonValuationTimeAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonValuationTimeAnnotation,
                   const_name("datetime.date | datetime.datetime"))
};

template <>
struct type_caster<kiyosi::python_binding::PythonGreekRequestAnnotation> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonGreekRequestAnnotation,
                   (kiyosi::python_binding::EnumNames<kiyosi::Greek>::Name +
                    const_name(" | collections.abc.Iterable[") +
                    kiyosi::python_binding::EnumNames<kiyosi::Greek>::Name +
                    const_name("]")))
};

template <typename Enum>
struct type_caster<kiyosi::python_binding::PythonChoice<Enum>> {
    NB_TYPE_CASTER(kiyosi::python_binding::PythonChoice<Enum>,
                   kiyosi::python_binding::EnumNames<Enum>::Name)
    bool from_python(handle source, uint32_t, cleanup_list*) noexcept
    {
        if (!isinstance<str>(source)) return false;
        value.source = source;
        return true;
    }
    static handle from_cpp(const kiyosi::python_binding::PythonChoice<Enum>& source,
                           rv_policy, cleanup_list*) noexcept
    {
        return source.source.inc_ref();
    }
};

template <typename Enum>
    requires requires { kiyosi::python_binding::EnumNames<Enum>::values; }
struct type_caster<Enum, enable_if_t<std::is_enum_v<Enum>>> {
    NB_TYPE_CASTER(Enum, kiyosi::python_binding::EnumNames<Enum>::Name)
    bool from_python(handle source, uint32_t, cleanup_list*) noexcept
    {
        if (!isinstance<str>(source)) return false;
        Py_ssize_t size;
        const char* data = PyUnicode_AsUTF8AndSize(source.ptr(), &size);
        if (!data) {
            PyErr_Clear();
            return false;
        }
        const std::string_view name{data, static_cast<std::size_t>(size)};
        for (const auto& [candidate, member] : kiyosi::python_binding::EnumNames<Enum>::values) {
            if (name == candidate) {
                value = member;
                return true;
            }
        }
        return false;
    }
    static handle from_cpp(Enum source, rv_policy, cleanup_list*)
    {
        for (const auto& [name, member] : kiyosi::python_binding::EnumNames<Enum>::values)
            if (source == member)
                return PyUnicode_FromStringAndSize(name.data(), static_cast<Py_ssize_t>(name.size()));
        PyErr_SetString(PyExc_RuntimeError, "unmapped core enum value");
        return {};
    }
};

} // namespace nanobind::detail

namespace kiyosi::python_binding {

using PythonReal = nb::typed<nb::handle, double>;
using PythonInteger = nb::typed<nb::handle, int>;
using PythonDate = nb::typed<nb::handle, PythonDateAnnotation>;
using PythonValuationTime = nb::typed<nb::handle, PythonValuationTimeAnnotation>;
using PythonGreekRequest = nb::typed<nb::handle, PythonGreekRequestAnnotation>;
using PythonRealSequence = nb::typed<nb::handle, nb::typed<nb::iterable, double>>;
using PythonDateSequence =
    nb::typed<nb::handle, nb::typed<nb::iterable, PythonDateAnnotation>>;
using PythonDateObject = nb::typed<nb::object, PythonDateAnnotation>;
using PythonTimestampObject = nb::typed<nb::object, PythonTimestampAnnotation>;
using PythonDateList = nb::typed<nb::list, PythonDateAnnotation>;
using PythonDateIterator =
    nb::typed<nb::object, nb::typed<nb::iterator, PythonDateAnnotation>>;

template <typename T>
void bind_repr(nb::class_<T>& binding, const char* name,
               std::vector<const char*> fields)
{
    const std::string type_name{name};
    const std::vector<const char*> attributes{std::move(fields)};
    binding.def("__repr__", [type_name, attributes](const T& value) { // NOLINT(bugprone-exception-escape)
        const nb::object self = nb::cast(&value, nb::rv_policy::reference);
        nb::list parts;
        for (const auto* field : attributes) {
            parts.append(nb::str("{}={!r}").attr("format")(
                nb::str(field), self.attr(field)));
        }
        return nb::str("{}({})").attr("format")(
            nb::str(type_name.c_str()), nb::str(", ").attr("join")(parts));
    });
}

template <typename T>
void bind_value_equality(nb::class_<T>& binding)
{
    binding.def("__eq__", [](const T& left, const T& right) { return left == right; }, nb::is_operator());
    binding.attr("__hash__") = nb::none();
}

class DomainException final : public std::runtime_error {
public:
    explicit DomainException(Error error)
        : std::runtime_error(error.message), category_(error.category) {}

    ErrorCategory category() const noexcept { return category_; }

private:
    ErrorCategory category_;
};

template <typename T>
T unwrap(Result<T> value)
{
    if (!value) throw DomainException{std::move(value.error())};
    return std::move(*value);
}

[[noreturn]] inline void type_error(std::string_view field, std::string_view expected)
{
    throw nb::type_error((std::string{field} + " must be " + std::string{expected}).c_str());
}

template <typename Enum>
Enum string_enum(nb::handle value, std::string_view field)
{
    if (!nb::isinstance<nb::str>(value)) type_error(field, "a string");
    Enum result{};
    if (!nb::try_cast(value, result, false)) throw nb::value_error("unknown string choice");
    return result;
}

template <typename Enum>
PythonChoice<Enum>::operator Enum() const
{
    return string_enum<Enum>(source, "choice");
}

inline double real_number(nb::handle value, std::string_view field)
{
    const nb::object real_type = nb::module_::import_("numbers").attr("Real");
    const int is_real = PyObject_IsInstance(value.ptr(), real_type.ptr());
    if (is_real < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_real == 0) type_error(field, "a real number");
    const double converted = PyFloat_AsDouble(value.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    return converted;
}

inline int integer(nb::handle value, std::string_view field)
{
    const nb::object integer_type = nb::module_::import_("numbers").attr("Integral");
    const int is_integer = PyObject_IsInstance(value.ptr(), integer_type.ptr());
    if (is_integer < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_integer == 0) type_error(field, "an integer");
    const long long converted = PyLong_AsLongLong(value.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    if (!std::in_range<int>(converted))
        throw std::overflow_error(std::string{field} + " is outside the range of a C++ int");
    return static_cast<int>(converted);
}

inline std::optional<std::uint64_t> optional_seed(nb::handle value)
{
    if (value.is_none()) return std::nullopt;
    const nb::object integer_type = nb::module_::import_("numbers").attr("Integral");
    const int is_integer = PyObject_IsInstance(value.ptr(), integer_type.ptr());
    if (is_integer < 0) throw nb::python_error();
    if (PyBool_Check(value.ptr()) || is_integer == 0)
        type_error("seed", "a non-negative integer or None");
    const nb::object index = nb::steal<nb::object>(PyNumber_Index(value.ptr()));
    if (!index.is_valid()) throw nb::python_error();
    const unsigned long long converted = PyLong_AsUnsignedLongLong(index.ptr());
    if (PyErr_Occurred()) throw nb::python_error();
    if (!std::in_range<std::uint64_t>(converted))
        throw std::overflow_error("seed is outside the range of a uint64_t");
    return static_cast<std::uint64_t>(converted);
}

inline Date calendar_date(nb::handle value, std::string_view field)
{
    const nb::object datetime_module = nb::module_::import_("datetime");
    const nb::object date_type = datetime_module.attr("date");
    const nb::object datetime_type = datetime_module.attr("datetime");
    const int is_date = PyObject_IsInstance(value.ptr(), date_type.ptr());
    const int is_datetime = PyObject_IsInstance(value.ptr(), datetime_type.ptr());
    if (is_date < 0 || is_datetime < 0) throw nb::python_error();
    if (is_date == 0 || is_datetime != 0) type_error(field, "a datetime.date");
    const nb::object object = nb::borrow<nb::object>(value);
    // Built-in descriptors read stored components, bypassing subclass properties that can narrow or wrap.
    return Date{std::chrono::year{nb::cast<int>(date_type.attr("year").attr("__get__")(object))} /
                std::chrono::month{nb::cast<unsigned>(date_type.attr("month").attr("__get__")(object))} /
                std::chrono::day{nb::cast<unsigned>(date_type.attr("day").attr("__get__")(object))}};
}

inline PythonDateObject python_date(Date value)
{
    const auto parts = std::chrono::year_month_day{value};
    const nb::object datetime = nb::module_::import_("datetime");
    return PythonDateObject{datetime.attr("date")(
        static_cast<int>(parts.year()), static_cast<unsigned>(parts.month()),
        static_cast<unsigned>(parts.day()))};
}

/// Returns a fresh Python list of date values; requires the GIL.
inline PythonDateList python_dates(std::span<const Date> values)
{
    PythonDateList output;
    for (const Date value : values) output.append(python_date(value));
    return output;
}

inline PythonTimestampObject python_timestamp(Timestamp value)
{
    const Date day = std::chrono::floor<std::chrono::days>(value);
    const std::chrono::hh_mm_ss time{
        std::chrono::floor<std::chrono::microseconds>(value - day)};
    const auto parts = std::chrono::year_month_day{day};
    const nb::object datetime = nb::module_::import_("datetime");
    return PythonTimestampObject{datetime.attr("datetime")(
        static_cast<int>(parts.year()), static_cast<unsigned>(parts.month()),
        static_cast<unsigned>(parts.day()), time.hours().count(), time.minutes().count(),
        time.seconds().count(), time.subseconds().count(),
        datetime.attr("timezone").attr("utc"))};
}

inline Timestamp valuation_time(nb::handle value)
{
    const nb::object datetime_module = nb::module_::import_("datetime");
    const nb::object datetime_type = datetime_module.attr("datetime");
    const int is_datetime = PyObject_IsInstance(value.ptr(), datetime_type.ptr());
    if (is_datetime < 0) throw nb::python_error();
    if (is_datetime == 0) return start_of_day(calendar_date(value, "valuation_time"));

    const nb::object object = nb::borrow<nb::object>(value);
    if (datetime_type.attr("utcoffset")(object).is_none())
        type_error("valuation_time", "a Date or timezone-aware datetime");
    const nb::object utc = datetime_type.attr("astimezone")(object, datetime_module.attr("timezone").attr("utc"));
    const Date day{std::chrono::year{nb::cast<int>(datetime_type.attr("year").attr("__get__")(utc))} /
                   std::chrono::month{nb::cast<unsigned>(datetime_type.attr("month").attr("__get__")(utc))} /
                   std::chrono::day{nb::cast<unsigned>(datetime_type.attr("day").attr("__get__")(utc))}};
    return start_of_day(day) + std::chrono::hours{nb::cast<int>(datetime_type.attr("hour").attr("__get__")(utc))} +
           std::chrono::minutes{nb::cast<int>(datetime_type.attr("minute").attr("__get__")(utc))} +
           std::chrono::seconds{nb::cast<int>(datetime_type.attr("second").attr("__get__")(utc))} +
           std::chrono::microseconds{nb::cast<int>(datetime_type.attr("microsecond").attr("__get__")(utc))};
}

inline std::vector<Date> date_sequence(nb::handle values, std::string_view field)
{
    std::vector<Date> output;
    for (nb::handle item : nb::borrow<nb::iterable>(values))
        output.push_back(calendar_date(item, field));
    return output;
}

inline std::vector<double> real_sequence(nb::handle values, std::string_view field)
{
    std::vector<double> output;
    for (nb::handle item : nb::borrow<nb::iterable>(values))
        output.push_back(real_number(item, field));
    return output;
}

void bind_error_category(nb::module_& module);
void bind_market(nb::module_& module);
void bind_instruments(nb::module_& module);
void bind_structured_instruments(nb::module_& module);
void bind_results(nb::module_& module);
void bind_engines(nb::module_& module);
void bind_analytics(nb::module_& module);

} // namespace kiyosi::python_binding
