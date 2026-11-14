import json
import os
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Literal

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


def _call(base, method, path, body, headers=None):
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


def reset():
    for c in CUSTOMERS:
        _call("WEFT_ADDRESSES_URL", "PUT", f"/addresses/{c['address']}", {"owner": c["customer"], "street": c["street"]})
        _call("WEFT_ORDERS_URL", "PUT", f"/orders/{c['order']}", {"customer": c["customer"], "address_id": c["address"]})
    shop.address_id = "addr-ana"


@dataclass
class Choice:
    address_id: Literal["addr-ana", "addr-ben"]


def choose_address(choice: Choice):
    _call("WEFT_ORDERS_URL", "PATCH", "/orders/o-ana", {"address_id": choice.address_id}, {"x-customer": "ana"})
    shop.address_id = choice.address_id
