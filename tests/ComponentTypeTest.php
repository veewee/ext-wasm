<?php

declare(strict_types=1);

namespace Test;

use PHPUnit\Framework\TestCase;
use Wasm\Component\Component;
use Wasm\Component\Instance;
use Wasm\Component\Type\FunctionType;
use Wasm\Component\Type\ValueType;

/**
 * Reflection of the WIT types of the component in ComponentValueTest, which
 * names person, maybe, color, perms, outcome and thing.
 */
final class ComponentTypeTest extends TestCase
{
    private static function component(): Component
    {
        $wat = (new \ReflectionClassConstant(ComponentValueTest::class, 'COMPONENT'))->getValue();

        return new Component($wat);
    }

    private static function signature(string $function): FunctionType
    {
        foreach (self::component()->exports() as $export) {
            if ($export['name'] === $function) {
                return $export['signature'];
            }
        }
        self::fail("no export $function");
    }

    public function test_a_function_signature_lists_its_parameters_and_result(): void
    {
        $check = self::signature('check');

        self::assertSame(['n'], array_keys($check->params));
        self::assertSame('u32', $check->params['n']->kind);
        self::assertSame('result', $check->result->kind);
        self::assertSame('u32', $check->result->ok->kind);
        self::assertSame('string', $check->result->err->kind);
    }

    public function test_a_function_without_result_has_none(): void
    {
        self::assertNull(self::signature('take-thing')->result);
    }

    public function test_a_named_record_carries_its_name_and_fields(): void
    {
        $person = self::signature('id-person')->params['v'];

        self::assertSame('record', $person->kind);
        self::assertSame('person', $person->name);
        self::assertSame(['first-name', 'age'], array_keys($person->fields));
        self::assertSame('string', $person->fields['first-name']->kind);
        self::assertSame('option', $person->fields['age']->kind);
        self::assertSame('u8', $person->fields['age']->element->kind);
    }

    public function test_variants_enums_and_flags_describe_their_cases(): void
    {
        $maybe = self::signature('id-maybe')->params['v'];
        self::assertSame(['variant', 'maybe'], [$maybe->kind, $maybe->name]);
        self::assertNull($maybe->cases['none']);
        self::assertSame('u32', $maybe->cases['some']->kind);

        $color = self::signature('id-color')->params['v'];
        self::assertSame(['enum', 'color', ['red', 'dark-blue']], [$color->kind, $color->name, $color->names]);

        $perms = self::signature('id-perms')->params['v'];
        self::assertSame(['flags', 'perms', ['read', 'write-all']], [$perms->kind, $perms->name, $perms->names]);
    }

    public function test_lists_tuples_and_resources_describe_what_they_hold(): void
    {
        $list = self::signature('id-strings')->params['v'];
        self::assertSame(['list', null, 'string'], [$list->kind, $list->name, $list->element->kind]);

        $tuple = self::signature('id-tuple')->params['v'];
        self::assertSame(['u32', 'string'], array_map(fn (ValueType $type): string => $type->kind, $tuple->types));

        $thing = self::signature('take-thing')->params['t'];
        self::assertSame(['own', 'thing'], [$thing->kind, $thing->resource]);
    }

    public function test_signature_strings_use_the_names(): void
    {
        self::assertSame('func(v: person) -> person', (string) self::signature('id-person'));
        self::assertSame('func(t: own<thing>)', (string) self::signature('take-thing'));

        $types = array_column(self::component()->exports(), 'type', 'name');
        self::assertSame('func(v: color) -> color', $types['id-color']);
    }

    public function test_a_func_of_an_instance_has_its_type(): void
    {
        $func = (new Instance(self::component()))->exports->get('id-person');

        self::assertSame('func(v: person) -> person', (string) $func->type());
    }

    public function test_types_cannot_be_changed(): void
    {
        $type = self::signature('check')->params['n'];

        // ext-php-rs reports a write to a getter-only property as an \Exception.
        $this->expectException(\Exception::class);
        $this->expectExceptionMessage('No setter');
        $type->kind = 'string';
    }

    public function test_two_names_for_the_same_shape_leave_the_type_nameless(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (core module $m (func (export "f") (param i32)))
              (core instance $i (instantiate $m))
              (type $a' (enum "x" "y"))
              (export $a "first" (type $a'))
              (type $b' (enum "x" "y"))
              (export $b "second" (type $b'))
              (func (export "take") (param "v" $a) (canon lift (core func $i "f"))))
            WAT);
        $take = array_column($component->exports(), 'signature', 'name')['take'];

        self::assertNull($take->params['v']->name);
        self::assertSame('func(v: enum { x, y })', (string) $take);
    }

    public function test_a_named_type_holding_a_resource_is_described_without_its_name(): void
    {
        $component = new Component(<<<'WAT'
            (component
              (core module $m (func (export "f") (param i32)))
              (core instance $i (instantiate $m))
              (type $thing' (resource (rep i32)))
              (export $thing "thing" (type $thing'))
              (type $holder' (record (field "t" (own $thing))))
              (export $holder "holder" (type $holder'))
              (type $other' (record (field "u" (own $thing))))
              (export $other "other" (type $other'))
              (func (export "take") (param "h" $holder) (canon lift (core func $i "f"))))
            WAT);
        $holder = array_column($component->exports(), 'signature', 'name')['take']->params['h'];

        self::assertSame(['record', null], [$holder->kind, $holder->name]);
        self::assertSame('thing', $holder->fields['t']->resource);
    }

    public function test_types_inside_an_interface_are_named_by_the_interface(): void
    {
        $counters = Component::fromFile(__DIR__ . '/fixtures/component-resources/counters.wasm')->exports()[0];
        $functions = array_column($counters['functions'], 'type', 'name');

        self::assertSame('func(self: borrow<counter>) -> u32', $functions['[method]counter.value']);
        self::assertSame('func(a: borrow<counter>, b: borrow<counter>) -> u32', $functions['total']);
    }

    public function test_a_func_with_resource_parameters_has_its_type(): void
    {
        $instance = new Instance(Component::fromFile(__DIR__ . '/fixtures/component-resources/counters.wasm'), wasi: new \Wasm\Wasi());
        $total = $instance->exports->get('docs:demo/counters')->get('total');

        self::assertSame('func(a: borrow<counter>, b: borrow<counter>) -> u32', (string) $total->type());
        self::assertSame('counter', $total->type()->params['a']->resource);
    }
}
