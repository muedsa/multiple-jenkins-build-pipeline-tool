<!-- Release Pipeline icon - master 512x512
     隐喻：三个递进圆角块 = 管线阶段（完成 / 进行中 / 发布）
           右上 45° 箭头 = 沿对角线继续向上发布

     箭头构成（沿屏幕 45° 轴对称，便于精确计算）：
       尖端 T = (396, 96)
       箭杆 = 竖直圆角条 w34 h108，整体绕自身中心顺时针旋转 45°
               旋转后其上端落到 T，下端落到左下尾
       横翼 = 从 T 向左的水平圆角条（外端 104 长）
       竖翼 = 从 T 向下的竖直圆角条
     横翼与竖翼在尖端各多伸出 8px，用于闭合接缝。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="512" height="512"
        borderRadius="116"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT"
        border="2 SOLID #FFFFFF26">

        <Stack>

            <!-- 阶段 1：已完成 -->
            <Positioned left="104" bottom="96" width="76" height="88">
                <Container color="#38BDF85C" borderRadius="22"/>
            </Positioned>

            <!-- 阶段 2：进行中 -->
            <Positioned left="218" bottom="96" width="76" height="140">
                <Container color="#22D3EEB3" borderRadius="22"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="332" bottom="96" width="76" height="192">
                <Container color="#67E8F9" borderRadius="22"/>
            </Positioned>

            <!-- 箭头横翼：从尖端向左 -->
            <Positioned left="292" top="79" width="104" height="34">
                <Container color="#F0F9FF" borderRadius="17"/>
            </Positioned>

            <!-- 箭头竖翼：从尖端向下 -->
            <Positioned left="379" top="88" width="34" height="104">
                <Container color="#F0F9FF" borderRadius="17"/>
            </Positioned>

            <!-- 箭杆：竖直条绕中心顺时针旋转 45°，上端正好落在尖端 -->
            <Positioned left="340.8" top="80.2" width="34" height="108">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="34" height="108" color="#F0F9FF" borderRadius="17"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>