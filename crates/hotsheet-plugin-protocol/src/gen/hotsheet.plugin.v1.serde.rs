impl serde::Serialize for Cancel {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Cancel", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Cancel {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "id" => Ok(GeneratedField::Id),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Cancel;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Cancel")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Cancel, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(Cancel {
                    id: id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Cancel", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Capability {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.features.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Capability", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.features.is_empty() {
            struct_ser.serialize_field("features", &self.features)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Capability {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "features",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Features,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "name" => Ok(GeneratedField::Name),
                            "features" => Ok(GeneratedField::Features),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Capability;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Capability")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Capability, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut features__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Features => {
                            if features__.is_some() {
                                return Err(serde::de::Error::duplicate_field("features"));
                            }
                            features__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(Capability {
                    name: name__.unwrap_or_default(),
                    features: features__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Capability", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CountItem {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.seq != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.CountItem", len)?;
        if self.seq != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("seq", ToString::to_string(&self.seq).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CountItem {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "seq",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Seq,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "seq" => Ok(GeneratedField::Seq),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CountItem;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.CountItem")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CountItem, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut seq__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Seq => {
                            if seq__.is_some() {
                                return Err(serde::de::Error::duplicate_field("seq"));
                            }
                            seq__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(CountItem {
                    seq: seq__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.CountItem", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CountRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.count != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.CountRequest", len)?;
        if self.count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("count", ToString::to_string(&self.count).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CountRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "count",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Count,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "count" => Ok(GeneratedField::Count),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CountRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.CountRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CountRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut count__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Count => {
                            if count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("count"));
                            }
                            count__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(CountRequest {
                    count: count__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.CountRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CountResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.sent != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.CountResponse", len)?;
        if self.sent != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("sent", ToString::to_string(&self.sent).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CountResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "sent",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Sent,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "sent" => Ok(GeneratedField::Sent),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CountResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.CountResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CountResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut sent__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Sent => {
                            if sent__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sent"));
                            }
                            sent__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(CountResponse {
                    sent: sent__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.CountResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EchoRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.text.is_empty() {
            len += 1;
        }
        if !self.data.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.EchoRequest", len)?;
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        if !self.data.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("data", pbjson::private::base64::encode(&self.data).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EchoRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "text",
            "data",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Text,
            Data,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "text" => Ok(GeneratedField::Text),
                            "data" => Ok(GeneratedField::Data),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EchoRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.EchoRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EchoRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut text__ = None;
                let mut data__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Data => {
                            if data__.is_some() {
                                return Err(serde::de::Error::duplicate_field("data"));
                            }
                            data__ =
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(EchoRequest {
                    text: text__.unwrap_or_default(),
                    data: data__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.EchoRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EchoResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.text.is_empty() {
            len += 1;
        }
        if !self.data.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.EchoResponse", len)?;
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        if !self.data.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("data", pbjson::private::base64::encode(&self.data).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EchoResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "text",
            "data",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Text,
            Data,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "text" => Ok(GeneratedField::Text),
                            "data" => Ok(GeneratedField::Data),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EchoResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.EchoResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EchoResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut text__ = None;
                let mut data__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Data => {
                            if data__.is_some() {
                                return Err(serde::de::Error::duplicate_field("data"));
                            }
                            data__ =
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(EchoResponse {
                    text: text__.unwrap_or_default(),
                    data: data__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.EchoResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Empty {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Empty", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Empty {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                            Err(serde::de::Error::unknown_field(value, FIELDS))
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Empty;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Empty")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Empty, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(Empty {
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Empty", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EnvironmentResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.names.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.EnvironmentResponse", len)?;
        if !self.names.is_empty() {
            struct_ser.serialize_field("names", &self.names)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EnvironmentResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "names",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Names,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "names" => Ok(GeneratedField::Names),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EnvironmentResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.EnvironmentResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EnvironmentResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut names__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Names => {
                            if names__.is_some() {
                                return Err(serde::de::Error::duplicate_field("names"));
                            }
                            names__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(EnvironmentResponse {
                    names: names__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.EnvironmentResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ExitRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.status != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.ExitRequest", len)?;
        if self.status != 0 {
            struct_ser.serialize_field("status", &self.status)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ExitRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "status",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Status,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "status" => Ok(GeneratedField::Status),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ExitRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.ExitRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ExitRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut status__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(ExitRequest {
                    status: status__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.ExitRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FailRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.error.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.FailRequest", len)?;
        if let Some(v) = self.error.as_ref() {
            struct_ser.serialize_field("error", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FailRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Error,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "error" => Ok(GeneratedField::Error),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FailRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.FailRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FailRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut error__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = map_.next_value()?;
                        }
                    }
                }
                Ok(FailRequest {
                    error: error__,
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.FailRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Frame {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.kind.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Frame", len)?;
        if let Some(v) = self.kind.as_ref() {
            match v {
                frame::Kind::Request(v) => {
                    struct_ser.serialize_field("request", v)?;
                }
                frame::Kind::Response(v) => {
                    struct_ser.serialize_field("response", v)?;
                }
                frame::Kind::StreamItem(v) => {
                    struct_ser.serialize_field("streamItem", v)?;
                }
                frame::Kind::Cancel(v) => {
                    struct_ser.serialize_field("cancel", v)?;
                }
                frame::Kind::Log(v) => {
                    struct_ser.serialize_field("log", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Frame {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "request",
            "response",
            "stream_item",
            "streamItem",
            "cancel",
            "log",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Request,
            Response,
            StreamItem,
            Cancel,
            Log,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "request" => Ok(GeneratedField::Request),
                            "response" => Ok(GeneratedField::Response),
                            "streamItem" | "stream_item" => Ok(GeneratedField::StreamItem),
                            "cancel" => Ok(GeneratedField::Cancel),
                            "log" => Ok(GeneratedField::Log),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Frame;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Frame")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Frame, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut kind__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Request => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("request"));
                            }
                            kind__ = map_.next_value::<::std::option::Option<_>>()?.map(frame::Kind::Request)
;
                        }
                        GeneratedField::Response => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("response"));
                            }
                            kind__ = map_.next_value::<::std::option::Option<_>>()?.map(frame::Kind::Response)
;
                        }
                        GeneratedField::StreamItem => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("streamItem"));
                            }
                            kind__ = map_.next_value::<::std::option::Option<_>>()?.map(frame::Kind::StreamItem)
;
                        }
                        GeneratedField::Cancel => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cancel"));
                            }
                            kind__ = map_.next_value::<::std::option::Option<_>>()?.map(frame::Kind::Cancel)
;
                        }
                        GeneratedField::Log => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("log"));
                            }
                            kind__ = map_.next_value::<::std::option::Option<_>>()?.map(frame::Kind::Log)
;
                        }
                    }
                }
                Ok(Frame {
                    kind: kind__,
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Frame", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Hello {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.protocol_versions.is_empty() {
            len += 1;
        }
        if !self.transports.is_empty() {
            len += 1;
        }
        if self.host.is_some() {
            len += 1;
        }
        if !self.plugin_id.is_empty() {
            len += 1;
        }
        if !self.data_dir.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Hello", len)?;
        if !self.protocol_versions.is_empty() {
            struct_ser.serialize_field("protocolVersions", &self.protocol_versions)?;
        }
        if !self.transports.is_empty() {
            struct_ser.serialize_field("transports", &self.transports)?;
        }
        if let Some(v) = self.host.as_ref() {
            struct_ser.serialize_field("host", v)?;
        }
        if !self.plugin_id.is_empty() {
            struct_ser.serialize_field("pluginId", &self.plugin_id)?;
        }
        if !self.data_dir.is_empty() {
            struct_ser.serialize_field("dataDir", &self.data_dir)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Hello {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "protocol_versions",
            "protocolVersions",
            "transports",
            "host",
            "plugin_id",
            "pluginId",
            "data_dir",
            "dataDir",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ProtocolVersions,
            Transports,
            Host,
            PluginId,
            DataDir,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "protocolVersions" | "protocol_versions" => Ok(GeneratedField::ProtocolVersions),
                            "transports" => Ok(GeneratedField::Transports),
                            "host" => Ok(GeneratedField::Host),
                            "pluginId" | "plugin_id" => Ok(GeneratedField::PluginId),
                            "dataDir" | "data_dir" => Ok(GeneratedField::DataDir),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Hello;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Hello")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Hello, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut protocol_versions__ = None;
                let mut transports__ = None;
                let mut host__ = None;
                let mut plugin_id__ = None;
                let mut data_dir__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ProtocolVersions => {
                            if protocol_versions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("protocolVersions"));
                            }
                            protocol_versions__ =
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Transports => {
                            if transports__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transports"));
                            }
                            transports__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = map_.next_value()?;
                        }
                        GeneratedField::PluginId => {
                            if plugin_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pluginId"));
                            }
                            plugin_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DataDir => {
                            if data_dir__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataDir"));
                            }
                            data_dir__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(Hello {
                    protocol_versions: protocol_versions__.unwrap_or_default(),
                    transports: transports__.unwrap_or_default(),
                    host: host__,
                    plugin_id: plugin_id__.unwrap_or_default(),
                    data_dir: data_dir__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Hello", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for HelloAck {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.protocol_version != 0 {
            len += 1;
        }
        if !self.transport.is_empty() {
            len += 1;
        }
        if self.plugin.is_some() {
            len += 1;
        }
        if !self.capabilities.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.HelloAck", len)?;
        if self.protocol_version != 0 {
            struct_ser.serialize_field("protocolVersion", &self.protocol_version)?;
        }
        if !self.transport.is_empty() {
            struct_ser.serialize_field("transport", &self.transport)?;
        }
        if let Some(v) = self.plugin.as_ref() {
            struct_ser.serialize_field("plugin", v)?;
        }
        if !self.capabilities.is_empty() {
            struct_ser.serialize_field("capabilities", &self.capabilities)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HelloAck {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "protocol_version",
            "protocolVersion",
            "transport",
            "plugin",
            "capabilities",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ProtocolVersion,
            Transport,
            Plugin,
            Capabilities,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "protocolVersion" | "protocol_version" => Ok(GeneratedField::ProtocolVersion),
                            "transport" => Ok(GeneratedField::Transport),
                            "plugin" => Ok(GeneratedField::Plugin),
                            "capabilities" => Ok(GeneratedField::Capabilities),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HelloAck;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.HelloAck")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HelloAck, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut protocol_version__ = None;
                let mut transport__ = None;
                let mut plugin__ = None;
                let mut capabilities__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ProtocolVersion => {
                            if protocol_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("protocolVersion"));
                            }
                            protocol_version__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Transport => {
                            if transport__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transport"));
                            }
                            transport__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Plugin => {
                            if plugin__.is_some() {
                                return Err(serde::de::Error::duplicate_field("plugin"));
                            }
                            plugin__ = map_.next_value()?;
                        }
                        GeneratedField::Capabilities => {
                            if capabilities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capabilities"));
                            }
                            capabilities__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(HelloAck {
                    protocol_version: protocol_version__.unwrap_or_default(),
                    transport: transport__.unwrap_or_default(),
                    plugin: plugin__,
                    capabilities: capabilities__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.HelloAck", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for HostInfo {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.HostInfo", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HostInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "version",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Version,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HostInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.HostInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HostInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut version__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(HostInfo {
                    name: name__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.HostInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Log {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.level.is_empty() {
            len += 1;
        }
        if !self.message.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Log", len)?;
        if !self.level.is_empty() {
            struct_ser.serialize_field("level", &self.level)?;
        }
        if !self.message.is_empty() {
            struct_ser.serialize_field("message", &self.message)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Log {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "level",
            "message",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Level,
            Message,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "level" => Ok(GeneratedField::Level),
                            "message" => Ok(GeneratedField::Message),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Log;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Log")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Log, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut level__ = None;
                let mut message__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Level => {
                            if level__.is_some() {
                                return Err(serde::de::Error::duplicate_field("level"));
                            }
                            level__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(Log {
                    level: level__.unwrap_or_default(),
                    message: message__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Log", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PluginError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.code.is_empty() {
            len += 1;
        }
        if !self.message.is_empty() {
            len += 1;
        }
        if self.retry_after_seconds.is_some() {
            len += 1;
        }
        if self.stale {
            len += 1;
        }
        if !self.capability.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.PluginError", len)?;
        if !self.code.is_empty() {
            struct_ser.serialize_field("code", &self.code)?;
        }
        if !self.message.is_empty() {
            struct_ser.serialize_field("message", &self.message)?;
        }
        if let Some(v) = self.retry_after_seconds.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("retryAfterSeconds", ToString::to_string(&v).as_str())?;
        }
        if self.stale {
            struct_ser.serialize_field("stale", &self.stale)?;
        }
        if !self.capability.is_empty() {
            struct_ser.serialize_field("capability", &self.capability)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PluginError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "code",
            "message",
            "retry_after_seconds",
            "retryAfterSeconds",
            "stale",
            "capability",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Code,
            Message,
            RetryAfterSeconds,
            Stale,
            Capability,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "code" => Ok(GeneratedField::Code),
                            "message" => Ok(GeneratedField::Message),
                            "retryAfterSeconds" | "retry_after_seconds" => Ok(GeneratedField::RetryAfterSeconds),
                            "stale" => Ok(GeneratedField::Stale),
                            "capability" => Ok(GeneratedField::Capability),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PluginError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.PluginError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PluginError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut code__ = None;
                let mut message__ = None;
                let mut retry_after_seconds__ = None;
                let mut stale__ = None;
                let mut capability__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Code => {
                            if code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("code"));
                            }
                            code__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RetryAfterSeconds => {
                            if retry_after_seconds__.is_some() {
                                return Err(serde::de::Error::duplicate_field("retryAfterSeconds"));
                            }
                            retry_after_seconds__ =
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Stale => {
                            if stale__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stale"));
                            }
                            stale__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Capability => {
                            if capability__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capability"));
                            }
                            capability__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(PluginError {
                    code: code__.unwrap_or_default(),
                    message: message__.unwrap_or_default(),
                    retry_after_seconds: retry_after_seconds__,
                    stale: stale__.unwrap_or_default(),
                    capability: capability__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.PluginError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PluginInfo {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.version.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.PluginInfo", len)?;
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PluginInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "version" => Ok(GeneratedField::Version),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PluginInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.PluginInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PluginInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(PluginInfo {
                    version: version__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.PluginInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Request {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if !self.method.is_empty() {
            len += 1;
        }
        if !self.payload.is_empty() {
            len += 1;
        }
        if self.deadline_ms != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Request", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if !self.method.is_empty() {
            struct_ser.serialize_field("method", &self.method)?;
        }
        if !self.payload.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("payload", pbjson::private::base64::encode(&self.payload).as_str())?;
        }
        if self.deadline_ms != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("deadlineMs", ToString::to_string(&self.deadline_ms).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Request {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "method",
            "payload",
            "deadline_ms",
            "deadlineMs",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Method,
            Payload,
            DeadlineMs,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "id" => Ok(GeneratedField::Id),
                            "method" => Ok(GeneratedField::Method),
                            "payload" => Ok(GeneratedField::Payload),
                            "deadlineMs" | "deadline_ms" => Ok(GeneratedField::DeadlineMs),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Request;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Request")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Request, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut method__ = None;
                let mut payload__ = None;
                let mut deadline_ms__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Method => {
                            if method__.is_some() {
                                return Err(serde::de::Error::duplicate_field("method"));
                            }
                            method__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Payload => {
                            if payload__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payload"));
                            }
                            payload__ =
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DeadlineMs => {
                            if deadline_ms__.is_some() {
                                return Err(serde::de::Error::duplicate_field("deadlineMs"));
                            }
                            deadline_ms__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(Request {
                    id: id__.unwrap_or_default(),
                    method: method__.unwrap_or_default(),
                    payload: payload__.unwrap_or_default(),
                    deadline_ms: deadline_ms__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Request", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Response {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if self.outcome.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.Response", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if let Some(v) = self.outcome.as_ref() {
            match v {
                response::Outcome::Payload(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("payload", pbjson::private::base64::encode(&v).as_str())?;
                }
                response::Outcome::Error(v) => {
                    struct_ser.serialize_field("error", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Response {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "payload",
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Payload,
            Error,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "id" => Ok(GeneratedField::Id),
                            "payload" => Ok(GeneratedField::Payload),
                            "error" => Ok(GeneratedField::Error),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Response;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.Response")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Response, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut outcome__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Payload => {
                            if outcome__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payload"));
                            }
                            outcome__ = map_.next_value::<::std::option::Option<::pbjson::private::BytesDeserialize<_>>>()?.map(|x| response::Outcome::Payload(x.0));
                        }
                        GeneratedField::Error => {
                            if outcome__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            outcome__ = map_.next_value::<::std::option::Option<_>>()?.map(response::Outcome::Error)
;
                        }
                    }
                }
                Ok(Response {
                    id: id__.unwrap_or_default(),
                    outcome: outcome__,
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.Response", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SleepRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.millis != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.SleepRequest", len)?;
        if self.millis != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("millis", ToString::to_string(&self.millis).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SleepRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "millis",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Millis,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "millis" => Ok(GeneratedField::Millis),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SleepRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.SleepRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SleepRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut millis__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Millis => {
                            if millis__.is_some() {
                                return Err(serde::de::Error::duplicate_field("millis"));
                            }
                            millis__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(SleepRequest {
                    millis: millis__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.SleepRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for StreamItem {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if !self.payload.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("hotsheet.plugin.v1.StreamItem", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if !self.payload.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("payload", pbjson::private::base64::encode(&self.payload).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for StreamItem {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "payload",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Payload,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "id" => Ok(GeneratedField::Id),
                            "payload" => Ok(GeneratedField::Payload),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = StreamItem;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct hotsheet.plugin.v1.StreamItem")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<StreamItem, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut payload__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ =
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Payload => {
                            if payload__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payload"));
                            }
                            payload__ =
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                    }
                }
                Ok(StreamItem {
                    id: id__.unwrap_or_default(),
                    payload: payload__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("hotsheet.plugin.v1.StreamItem", FIELDS, GeneratedVisitor)
    }
}
