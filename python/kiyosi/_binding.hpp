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
#include <stdexcept>
#include <string>
#include <string_view>
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

#define KIYOSI_STRING_ENUM_CASTER(Enum)                                                     \
    template <>                                                                             \
    struct type_caster<kiyosi::Enum> {                                                      \
        NB_TYPE_CASTER(kiyosi::Enum, kiyosi::python_binding::EnumNames<kiyosi::Enum>::Name) \
        bool from_python(handle source, uint32_t, cleanup_list*) noexcept                   \
        {                                                                                   \
            if (!isinstance<str>(source)) return false;                                     \
            Py_ssize_t size;                                                                \
            const char* data = PyUnicode_AsUTF8AndSize(source.ptr(), &size);                \
            if (!data) {                                                                    \
                PyErr_Clear();                                                              \
                return false;                                                               \
            }                                                                               \
            const std::string_view name{data, static_cast<std::size_t>(size)};              \
            for (const auto& [candidate, member] :                                          \
                 kiyosi::python_binding::EnumNames<kiyosi::Enum>::values) {                 \
                if (name == candidate) {                                                    \
                    value = member;                                                         \
                    return true;                                                            \
                }                                                                           \
            }                                                                               \
            PyErr_SetString(PyExc_ValueError, "unknown " #Enum " value");                   \
            return false;                                                                   \
        }                                                                                   \
        static handle from_cpp(kiyosi::Enum source, rv_policy, cleanup_list*)               \
        {                                                                                   \
            for (const auto& [name, member] :                                               \
                 kiyosi::python_binding::EnumNames<kiyosi::Enum>::values)                   \
                if (source == member)                                                       \
                    return PyUnicode_FromStringAndSize(                                     \
                        name.data(), static_cast<Py_ssize_t>(name.size()));                 \
            PyErr_SetString(PyExc_RuntimeError, "unmapped core " #Enum " value");           \
            return {};                                                                      \
        }                                                                                   \
    };

KIYOSI_STRING_ENUM_CASTER(OptionType)
KIYOSI_STRING_ENUM_CASTER(BarrierType)
KIYOSI_STRING_ENUM_CASTER(ObservationMode)
KIYOSI_STRING_ENUM_CASTER(BarrierTouchState)
KIYOSI_STRING_ENUM_CASTER(RebateTiming)
KIYOSI_STRING_ENUM_CASTER(SettlementTiming)
KIYOSI_STRING_ENUM_CASTER(PayoffType)
KIYOSI_STRING_ENUM_CASTER(KnockInObservationMode)
KIYOSI_STRING_ENUM_CASTER(AutocallableBarrierState)
KIYOSI_STRING_ENUM_CASTER(FiniteDifferenceScheme)
KIYOSI_STRING_ENUM_CASTER(MonteCarloBackend)
KIYOSI_STRING_ENUM_CASTER(CouponQuoteConvention)
KIYOSI_STRING_ENUM_CASTER(BusinessDayConvention)
KIYOSI_STRING_ENUM_CASTER(Greek)

#undef KIYOSI_STRING_ENUM_CASTER

} // namespace nanobind::detail

namespace kiyosi::python_binding {

using PythonReal = nb::typed<nb::handle, double>;
using PythonInteger = nb::typed<nb::handle, int>;
using PythonDate = nb::typed<nb::handle, PythonDateAnnotation>;
using PythonValuationTime = nb::typed<nb::handle, PythonValuationTimeAnnotation>;
using PythonGreekRequest = nb::typed<nb::handle, PythonGreekRequestAnnotation>;
using PythonBackend = nb::typed<nb::handle, MonteCarloBackend>;
using PythonRealSequence = nb::typed<nb::handle, nb::typed<nb::iterable, double>>;
using PythonDateSequence =
    nb::typed<nb::handle, nb::typed<nb::iterable, PythonDateAnnotation>>;
using PythonDateObject = nb::typed<nb::object, PythonDateAnnotation>;
using PythonTimestampObject = nb::typed<nb::object, PythonTimestampAnnotation>;
using PythonDateList = nb::typed<nb::list, PythonDateAnnotation>;
using PythonDateIterator =
    nb::typed<nb::object, nb::typed<nb::iterator, PythonDateAnnotation>>;
using PythonOptionalReal = nb::typed<nb::object, std::optional<double>>;

struct ReprField {
    const char* name;
    const char* attribute;
};

template <typename T>
void bind_repr(nb::class_<T>& binding, const char* name,
               std::vector<ReprField> fields)
{
    const std::string type_name{name};
    const std::vector<ReprField> attributes{std::move(fields)};
    binding.def("__repr__", [type_name, attributes](const T& value) { // NOLINT(bugprone-exception-escape)
        const nb::object self = nb::cast(&value, nb::rv_policy::reference);
        nb::list parts;
        for (const auto& [field, attribute] : attributes) {
            parts.append(nb::str("{}={!r}").attr("format")(
                nb::str(field), self.attr(attribute)));
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

inline void unwrap(Result<void> value)
{
    if (!value) throw DomainException{std::move(value.error())};
}

[[noreturn]] inline void type_error(std::string_view field, std::string_view expected)
{
    throw nb::type_error((std::string{field} + " must be " + std::string{expected}).c_str());
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
    const unsigned long long converted = PyLong_AsUnsignedLongLong(value.ptr());
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
    return Date{std::chrono::year{nb::cast<int>(object.attr("year"))} /
                std::chrono::month{nb::cast<unsigned>(object.attr("month"))} /
                std::chrono::day{nb::cast<unsigned>(object.attr("day"))}};
}

inline PythonDateObject python_date(Date value)
{
    const auto parts = std::chrono::year_month_day{value};
    const nb::object datetime = nb::module_::import_("datetime");
    return PythonDateObject{datetime.attr("date")(
        static_cast<int>(parts.year()), static_cast<unsigned>(parts.month()),
        static_cast<unsigned>(parts.day()))};
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
    if (object.attr("utcoffset")().is_none())
        type_error("valuation_time", "a Date or timezone-aware datetime");
    const nb::object utc = object.attr("astimezone")(datetime_module.attr("timezone").attr("utc"));
    const Date day{std::chrono::year{nb::cast<int>(utc.attr("year"))} /
                   std::chrono::month{nb::cast<unsigned>(utc.attr("month"))} /
                   std::chrono::day{nb::cast<unsigned>(utc.attr("day"))}};
    return start_of_day(day) + std::chrono::hours{nb::cast<int>(utc.attr("hour"))} +
           std::chrono::minutes{nb::cast<int>(utc.attr("minute"))} +
           std::chrono::seconds{nb::cast<int>(utc.attr("second"))} +
           std::chrono::microseconds{nb::cast<int>(utc.attr("microsecond"))};
}

inline std::vector<Date> date_sequence(nb::handle values, std::string_view field)
{
    const nb::object iterator = nb::steal<nb::object>(PyObject_GetIter(values.ptr()));
    if (!iterator.is_valid()) throw nb::python_error();
    std::vector<Date> output;
    while (PyObject* item = PyIter_Next(iterator.ptr())) {
        const nb::object owned = nb::steal<nb::object>(item);
        output.push_back(calendar_date(owned, field));
    }
    if (PyErr_Occurred()) throw nb::python_error();
    return output;
}

inline std::vector<double> real_sequence(nb::handle values, std::string_view field)
{
    const nb::object iterator = nb::steal<nb::object>(PyObject_GetIter(values.ptr()));
    if (!iterator.is_valid()) throw nb::python_error();
    std::vector<double> output;
    while (PyObject* item = PyIter_Next(iterator.ptr())) {
        const nb::object owned = nb::steal<nb::object>(item);
        output.push_back(real_number(owned, field));
    }
    if (PyErr_Occurred()) throw nb::python_error();
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
