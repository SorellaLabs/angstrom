# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "eth-account==0.13.4",
# ]
# ///

from typing import Any, Dict, TypeAlias, Callable, Generator
import json
import argparse
import os
from time import time
from eth_account._utils.encode_typed_data.encoding_and_hashing import hash_struct


NestedDirs: TypeAlias = bool | Dict[str, 'NestedDirs']


def map_dir(current_path: str) -> Dict[str, 'NestedDirs']:
    sub_paths = os.listdir(current_path)
    return {
        sub_path:
        True if os.path.isfile(full_sub := os.path.join(current_path, sub_path))
        else map_dir(full_sub)
        for sub_path in sub_paths
    }


def matching_paths(
    out_dir: Dict[str, 'NestedDirs'],
    target_file: str,
    path_so_far: tuple[str, ...] = tuple()
) -> Generator[str, None, None]:
    for path_segment, sub_value in out_dir.items():
        if path_segment == target_file:
            yield os.path.join(*path_so_far, path_segment)
        elif isinstance(sub_value, Dict):
            yield from matching_paths(
                sub_value,
                target_file,
                path_so_far=path_so_far + (path_segment,)
            )


def path_match(target_path: str, match: str) -> int:
    for i in range(len(target_path)):
        if target_path[-i-1:] != match[-i-1:]:
            return i
    return len(target_path)


JsonObject: TypeAlias = Dict[str, 'Json']
Json: TypeAlias = bool | int | str | list['Json'] | JsonObject


def ast_get_all_nodes(ast: JsonObject, criteria: Callable[[JsonObject], bool]) -> Generator[JsonObject, None, None]:
    if criteria(ast):
        yield ast
    nodes = ast.get('nodes', [])
    assert isinstance(nodes, list)
    for node in nodes:
        assert isinstance(node, Dict)
        yield from ast_get_all_nodes(node, criteria)


def get(obj: Json, *path: str) -> Json:
    current_obj: Json = obj
    for step in path:
        assert isinstance(current_obj, dict), f'Expected object'
        current_obj = current_obj[step]
    return current_obj


def get_str(obj: Json, *path: str) -> str:
    out = get(obj, *path)
    assert isinstance(out, str), f'Expected string'
    return out


def get_list(obj: Json, *path: str) -> list[Json]:
    out = get(obj, *path)
    assert isinstance(out, list), f'Expected list'
    return list(out)


def member_to_eip712_field(member: Json) -> Dict[str, str]:
    name = get_str(member, 'name')
    node_type = get_str(member, 'typeName', 'nodeType')
    if node_type == 'UserDefinedTypeName':
        field_type = get_str(member, 'typeName', 'pathNode', 'name')
    elif node_type == 'ElementaryTypeName':
        field_type = get_str(member, 'typeName', 'name')
    else:
        raise ValueError(
            f'Unrecognized member .typeName.nodeType {node_type!r}'
        )

    return {'name': str(name), 'type': str(field_type)}


def ast_to_eip712_type(ast: JsonObject) -> tuple[str, list[Dict[str, str]]]:
    name = get_str(ast, 'name')
    fields = [
        member_to_eip712_field(member)
        for member in get_list(ast, 'members')
    ]
    if 'documentation' in ast:
        doc = ast['documentation']
        doc = get_str(doc, 'text')
        if doc.startswith('@custom:erc712'):
            cmd = doc.replace('@custom:erc712', '')
            if cmd.startswith(':'):
                sub_cmd, data = cmd[1:].split(None, 1)
                if sub_cmd == 'exclude':
                    exclude = [f.strip() for f in data.split(',')]
                    fields = [
                        f
                        for f in fields
                        if f['name'] not in exclude
                    ]
                else:
                    raise ValueError(
                        f'Unknown sub command {sub_cmd!r} (data: {data!r})'
                    )
            else:
                raise ValueError(f'Unknown command {cmd!r}')
    return name, fields


def parse_field_value(value: str, field_type: str) -> Any:
    if field_type == 'bool':
        if value in ('false', 'False', '0'):
            return False
        elif value in ('true', 'True', '1'):
            return True
        else:
            raise TypeError(f'Unrecognized boolean value "{value!r}"')
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('path_struct', type=str)
    parser.add_argument('--out', '-o', type=str, default='out')
    parser.add_argument('values', nargs='*')
    args = parser.parse_args()

    struct_path, struct_name = str(args.path_struct).split(':', 1)

    out_dir = map_dir(args.out)

    _, path_tail = os.path.split(struct_path)

    possibilities = matching_paths(
        out_dir, path_tail, path_so_far=(args.out,))
    artifacts_path = max(
        possibilities,
        key=lambda m: path_match(struct_path, m)
    )
    artifact_path = os.listdir(artifacts_path)[0]
    artifact_path = os.path.join(artifacts_path, artifact_path)

    with open(artifact_path, 'r') as f:
        ast = json.load(f)['ast']
    struct_defs = ast_get_all_nodes(
        ast,
        lambda node: node['nodeType'] == 'StructDefinition'
    )

    file_types = dict(
        ast_to_eip712_type(struct_def)
        for struct_def in struct_defs
    )

    fields = file_types[struct_name]

    if len(args.values) != len(fields):
        raise ValueError(
            f'Got {len(args.values)} values, expected {len(fields)} in:\n{json.dumps(fields)}'
        )

    hash = hash_struct(
        struct_name,
        file_types,
        {
            field['name']: parse_field_value(value, field['type'])
            for field, value in zip(fields, args.values)
        }
    )

    print(f'0x{hash.hex()}')


if __name__ == '__main__':
    main()
