import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass

import weft

CUSTOMERS = [
    {"customer": "ana", "order": "o-ana", "address": "addr-ana", "street": "1 Elm Street"},
    {"customer": "ben", "order": "o-ben", "address": "addr-ben", "street": "9 Oak Avenue"},
]


@dataclass
class Shop:
    address_id: str = ""


shop = Shop()
weft.state("shop", lambda: shop)

chooseAddressSchema = {
    "type": "object",
    "properties": {"address_id": {"type": "string", "enum": ["addr-ana", "addr-ben"]}},
    "required": ["address_id"],
    "additionalProperties": False,
    "weft": {"verb": "choose_address"},
}


def call(base, method, path, body, headers=None):
    request = urllib.request.Request(
        os.environ[base] + path,
        data=json.dumps(body).encode(),
        method=method,
        headers={"content-type": "application/json", **(headers or {})},
    )
    try:
        with urllib.request.urlopen(request) as response:
            return json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"{path}: {error.code} {error.read().decode()}") from error


def open_shop():
    for c in CUSTOMERS:
        call("WEFT_ADDRESSES_URL", "PUT", f"/addresses/{c['address']}", {"owner": c["customer"], "street": c["street"]})
        call("WEFT_ORDERS_URL", "PUT", f"/orders/{c['order']}", {"customer": c["customer"], "address_id": c["address"]})
    shop.address_id = "addr-ana"


def choose_address(input):
    call("WEFT_ORDERS_URL", "PATCH", "/orders/o-ana", {"address_id": input["address_id"]}, {"x-customer": "ana"})
    shop.address_id = input["address_id"]
